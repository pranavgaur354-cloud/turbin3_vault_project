use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_program},
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    litesvm::{types::TransactionResult, LiteSVM},
    q3_26_vault::{VaultState, STATE, VAULT_SEED},
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

const SOL: u64 = 1_000_000_000;
const FEE: u64 = 5_000; // LiteSVM default: 5,000 lamports per signature
const DEPOSIT: u64 = 500_000_000;
const WITHDRAW: u64 = 100_000_000;

// ---------- helpers ----------

struct Env {
    svm: LiteSVM,
    user: Keypair,
    vault_state: Pubkey,
    vault: Pubkey,
}

fn pdas(user: &Pubkey) -> (Pubkey, Pubkey) {
    let id = q3_26_vault::id();
    let (state, _) = Pubkey::find_program_address(&[STATE, user.as_ref()], &id);
    let (vault, _) = Pubkey::find_program_address(&[VAULT_SEED, user.as_ref()], &id);
    (state, vault)
}

fn setup() -> Env {
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/q3_26_vault.so"
    ));
    svm.add_program(q3_26_vault::id(), bytes).unwrap();

    let user = Keypair::new();
    svm.airdrop(&user.pubkey(), 10 * SOL).unwrap();
    let (vault_state, vault) = pdas(&user.pubkey());
    Env { svm, user, vault_state, vault }
}

fn send(svm: &mut LiteSVM, signer: &Keypair, ix: Instruction) -> TransactionResult {
    svm.expire_blockhash(); // fresh blockhash so identical txs are not rejected as duplicates
    let msg = Message::new_with_blockhash(&[ix], Some(&signer.pubkey()), &svm.latest_blockhash());
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[signer]).unwrap();
    svm.send_transaction(tx)
}

fn ok(res: TransactionResult) {
    if let Err(e) = res {
        panic!("tx failed: {:?}\nlogs: {:#?}", e.err, e.meta.logs);
    }
}

/// Asserts the tx failed and that `needle` (an error name) appears in the program logs.
fn fails_with(res: TransactionResult, needle: &str) {
    let e = res.expect_err("tx should have failed");
    assert!(
        e.meta.logs.iter().any(|l| l.contains(needle)),
        "expected `{needle}` in logs: {:#?}",
        e.meta.logs
    );
}

fn initialize_ix(user: Pubkey, vault_state: Pubkey, vault: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        q3_26_vault::id(),
        &q3_26_vault::instruction::Initialize {}.data(),
        q3_26_vault::accounts::Initialize { user, vault_state, vault, system_program: system_program::ID }
            .to_account_metas(None),
    )
}

fn deposit_ix(user: Pubkey, vault_state: Pubkey, vault: Pubkey, amount: u64) -> Instruction {
    Instruction::new_with_bytes(
        q3_26_vault::id(),
        &q3_26_vault::instruction::Deposit { amount }.data(),
        q3_26_vault::accounts::Deposit { user, vault_state, vault, system_program: system_program::ID }
            .to_account_metas(None),
    )
}

fn withdraw_ix(user: Pubkey, vault_state: Pubkey, vault: Pubkey, amount: u64) -> Instruction {
    Instruction::new_with_bytes(
        q3_26_vault::id(),
        &q3_26_vault::instruction::Withdraw { amount }.data(),
        q3_26_vault::accounts::Withdraw { user, vault_state, vault, system_program: system_program::ID }
            .to_account_metas(None),
    )
}

fn close_ix(user: Pubkey, vault_state: Pubkey, vault: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        q3_26_vault::id(),
        &q3_26_vault::instruction::Close {}.data(),
        q3_26_vault::accounts::Close { user, vault_state, vault, system_program: system_program::ID }
            .to_account_metas(None),
    )
}

/// setup + initialize + deposit DEPOSIT lamports
fn funded() -> Env {
    let mut e = setup();
    let u = e.user.pubkey();
    ok(send(&mut e.svm, &e.user, initialize_ix(u, e.vault_state, e.vault)));
    ok(send(&mut e.svm, &e.user, deposit_ix(u, e.vault_state, e.vault, DEPOSIT)));
    e
}

// ---------- initialize ----------

#[test]
fn initialize_creates_state_and_rent_exempt_vault() {
    let mut e = setup();
    let u = e.user.pubkey();
    ok(send(&mut e.svm, &e.user, initialize_ix(u, e.vault_state, e.vault)));

    let acc = e.svm.get_account(&e.vault_state).expect("vault_state should exist");
    let state = VaultState::try_deserialize(&mut acc.data.as_ref()).unwrap();
    let (_, vault_bump) = Pubkey::find_program_address(&[VAULT_SEED, u.as_ref()], &q3_26_vault::id());
    assert_eq!(state.vault_bump, vault_bump, "stored vault bump should match");

    let rent = e.svm.minimum_balance_for_rent_exemption(0);
    assert_eq!(e.svm.get_balance(&e.vault).unwrap(), rent, "vault holds exactly rent");
}

#[test]
fn initialize_twice_fails() {
    let mut e = setup();
    let u = e.user.pubkey();
    ok(send(&mut e.svm, &e.user, initialize_ix(u, e.vault_state, e.vault)));
    fails_with(send(&mut e.svm, &e.user, initialize_ix(u, e.vault_state, e.vault)), "already in use");
}

// ---------- deposit ----------

#[test]
fn deposit_moves_lamports_into_vault() {
    let e = funded();
    let rent = e.svm.minimum_balance_for_rent_exemption(0);
    assert_eq!(e.svm.get_balance(&e.vault).unwrap(), rent + DEPOSIT);
}

#[test]
fn deposit_zero_fails() {
    let mut e = setup();
    let u = e.user.pubkey();
    ok(send(&mut e.svm, &e.user, initialize_ix(u, e.vault_state, e.vault)));
    fails_with(send(&mut e.svm, &e.user, deposit_ix(u, e.vault_state, e.vault, 0)), "InvalidAmount");
}

// ---------- withdraw ----------

#[test]
fn withdraw_moves_lamports_back_to_user() {
    let mut e = funded();
    let u = e.user.pubkey();
    let rent = e.svm.minimum_balance_for_rent_exemption(0);
    let user_before = e.svm.get_balance(&u).unwrap();

    ok(send(&mut e.svm, &e.user, withdraw_ix(u, e.vault_state, e.vault, WITHDRAW)));

    assert_eq!(e.svm.get_balance(&e.vault).unwrap(), rent + DEPOSIT - WITHDRAW);
    assert_eq!(e.svm.get_balance(&u).unwrap(), user_before + WITHDRAW - FEE);
}

#[test]
fn withdraw_everything_above_rent_works() {
    let mut e = funded();
    let u = e.user.pubkey();
    let rent = e.svm.minimum_balance_for_rent_exemption(0);
    ok(send(&mut e.svm, &e.user, withdraw_ix(u, e.vault_state, e.vault, DEPOSIT)));
    assert_eq!(e.svm.get_balance(&e.vault).unwrap(), rent, "vault stays rent-exempt");
}

#[test]
fn withdraw_more_than_available_fails() {
    let mut e = funded();
    let u = e.user.pubkey();
    // 1 lamport more than the deposit would dip into the vault's rent reserve
    fails_with(
        send(&mut e.svm, &e.user, withdraw_ix(u, e.vault_state, e.vault, DEPOSIT + 1)),
        "InsufficientFunds",
    );
}

#[test]
fn withdraw_zero_fails() {
    let mut e = funded();
    let u = e.user.pubkey();
    fails_with(send(&mut e.svm, &e.user, withdraw_ix(u, e.vault_state, e.vault, 0)), "InvalidAmount");
}

#[test]
fn other_user_cannot_withdraw_from_my_vault() {
    let mut e = funded();
    let thief = Keypair::new();
    e.svm.airdrop(&thief.pubkey(), SOL).unwrap();
    // The thief signs but passes the victim's PDAs: seeds are derived from the
    // signer's key, so the constraint check rejects it.
    fails_with(
        send(&mut e.svm, &thief, withdraw_ix(thief.pubkey(), e.vault_state, e.vault, WITHDRAW)),
        "ConstraintSeeds",
    );
}

// ---------- close ----------

#[test]
fn close_returns_everything_and_removes_accounts() {
    let mut e = funded();
    let u = e.user.pubkey();
    let user_before = e.svm.get_balance(&u).unwrap();
    let vault_lamports = e.svm.get_balance(&e.vault).unwrap();
    let state_lamports = e.svm.get_balance(&e.vault_state).unwrap();

    ok(send(&mut e.svm, &e.user, close_ix(u, e.vault_state, e.vault)));

    assert!(e.svm.get_account(&e.vault_state).map_or(true, |a| a.lamports == 0), "vault_state closed");
    assert_eq!(e.svm.get_balance(&e.vault).unwrap_or(0), 0, "vault drained");
    assert_eq!(
        e.svm.get_balance(&u).unwrap(),
        user_before + vault_lamports + state_lamports - FEE,
        "user gets deposits + both rents back"
    );
}

#[test]
fn can_reinitialize_after_close() {
    let mut e = funded();
    let u = e.user.pubkey();
    ok(send(&mut e.svm, &e.user, close_ix(u, e.vault_state, e.vault)));
    ok(send(&mut e.svm, &e.user, initialize_ix(u, e.vault_state, e.vault)));
}

#[test]
fn other_user_cannot_close_my_vault() {
    let mut e = funded();
    let thief = Keypair::new();
    e.svm.airdrop(&thief.pubkey(), SOL).unwrap();
    fails_with(
        send(&mut e.svm, &thief, close_ix(thief.pubkey(), e.vault_state, e.vault)),
        "ConstraintSeeds",
    );
}
