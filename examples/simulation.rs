use fortis_sdk::{
    client::{
        multisig_create, proposal_accounts_close, proposal_approve, proposal_create,
        proposal_execute,
    },
    pda::{
        FORTIS_PROGRAM_ID, TREASURY, get_multisig_pda, get_proposal_pda, get_transaction_pda,
        get_vault_pda,
    },
    state::{
        MultisigCreateAccounts, MultisigCreateArgs, ProposalAccountsCloseAccounts,
        ProposalApproveAccounts, ProposalApproveArgs, ProposalCreateAccounts, ProposalCreateArgs,
        ProposalExecuteAccounts, ProposallExecuteArgs, VaultTransactionMessage,
    },
};

use litesvm::LiteSVM;
use solana_sdk::{
    message::Message, pubkey, pubkey::Pubkey, signature::Keypair, signer::Signer,
    transaction::Transaction,
};
use solana_system_interface::instruction::transfer as native_transfer;

pub const SYSTEM_PROGRAM_ID: Pubkey = pubkey!("11111111111111111111111111111111");
#[tokio::main]
pub async fn main() {
    // ------------------------------------------------------------------------------
    // 1. Initialize Local SVM Environment
    // ------------------------------------------------------------------------------
    let mut svm = LiteSVM::new();
    svm.add_program_from_file(FORTIS_PROGRAM_ID, "./fortis.so")
        .expect("Failed to load Fortis program");

    // Multisig members + "create key" seed
    let (member_1, member_2, member_3, create_key) = (
        Keypair::new(),
        Keypair::new(),
        Keypair::new(),
        Keypair::new(),
    );

    // Fund members
    for member in [&member_1, &member_2, &member_3] {
        svm.airdrop(&member.pubkey(), 1_000_000_000)
            .expect("Airdrop failed");
    }

    // ------------------------------------------------------------------------------
    // 2. Create Multisig
    // ------------------------------------------------------------------------------
    let multisig_pda = get_multisig_pda(&create_key.pubkey(), None).0;

    let threshold = 2u8;
    let members = vec![member_1.pubkey(), member_2.pubkey(), member_3.pubkey()];
    let creator = member_1.insecure_clone();

    let multisig_create_ix = multisig_create(
        MultisigCreateAccounts {
            treasury: TREASURY,
            multisig: multisig_pda,
            create_key: create_key.pubkey(),
            creator: creator.pubkey(),
            system_program: SYSTEM_PROGRAM_ID,
        },
        MultisigCreateArgs {
            threshold,
            rent_collector: Some(creator.pubkey()),
            members,
        },
        None,
    );

    let tx = Transaction::new(
        &[creator.insecure_clone(), create_key.insecure_clone()],
        Message::new(&[multisig_create_ix], Some(&creator.pubkey())),
        svm.latest_blockhash(),
    );

    println!(
        "Multisig Created Logs:\n{:#?}",
        svm.send_transaction(tx).unwrap().logs
    );

    // ------------------------------------------------------------------------------
    // 3. Transfer SOL into the Vault PDA
    // ------------------------------------------------------------------------------
    let vault_pda = get_vault_pda(&multisig_pda, None).0;

    let transfer_to_vault_ix = native_transfer(&creator.pubkey(), &vault_pda, 1_000_000);

    let tx = Transaction::new(
        &[creator.insecure_clone()],
        Message::new(&[transfer_to_vault_ix], Some(&creator.pubkey())),
        svm.latest_blockhash(),
    );

    println!(
        "Transfer To Vault Logs:\n{:#?}",
        svm.send_transaction(tx).unwrap().logs
    );

    // ------------------------------------------------------------------------------
    // 4. Create Proposal for Vault Transaction
    // ------------------------------------------------------------------------------
    let transaction_index = 1u64;

    let transaction_pda = get_transaction_pda(&multisig_pda, transaction_index, None).0;
    let proposal_pda = get_proposal_pda(&multisig_pda, transaction_index, None).0;

    let proposal_accounts = ProposalCreateAccounts {
        multisig: multisig_pda,
        trasaction: transaction_pda,
        creator: creator.pubkey(),
        proposal: proposal_pda,
        system_program: SYSTEM_PROGRAM_ID,
    };

    let voting_deadline = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + (86400 * 30); // 30 days

    // Vault action inside the proposal
    let receiver = Keypair::new();
    let vault_transfer_ix = native_transfer(&vault_pda, &receiver.pubkey(), 1_000_000);

    let vault_message =
        VaultTransactionMessage::try_compile(&vault_pda, &[vault_transfer_ix.clone()], &[])
            .expect("Failed to compile vault message");

    let proposal_create_ix = proposal_create(
        proposal_accounts,
        0,              // num_ephemeral_signers
        &vault_message, // transaction message
        voting_deadline,
        None,
    );

    let tx = Transaction::new(
        &[creator.insecure_clone()],
        Message::new(&[proposal_create_ix], Some(&creator.pubkey())),
        svm.latest_blockhash(),
    );

    println!(
        "Proposal Created Logs:\n{:#?}",
        svm.send_transaction(tx).unwrap().logs
    );

    // ------------------------------------------------------------------------------
    // 5. Members approve the proposal
    // ------------------------------------------------------------------------------
    for member in [&member_2, &member_3] {
        let approve_ix = proposal_approve(
            ProposalApproveAccounts {
                multisig: multisig_pda,
                proposal: proposal_pda,
                member: member.pubkey(),
            },
            ProposalApproveArgs {},
            None,
        );

        let tx = Transaction::new(
            &[member.insecure_clone()],
            Message::new(&[approve_ix], Some(&member.pubkey())),
            svm.latest_blockhash(),
        );

        println!(
            "Proposal Approve (member {}) Logs:\n{:#?}",
            member.pubkey(),
            svm.send_transaction(tx).unwrap().logs
        );
    }
    // ------------------------------------------------------------------------------
    // 6. Execute Proposal Transaction (after threshold approvals)
    // ------------------------------------------------------------------------------
    let proposal_execute_ix = proposal_execute(
        &svm.get_account(&transaction_pda)
            .expect("transaction PDA missing")
            .data,
        ProposalExecuteAccounts {
            multisig: multisig_pda,
            proposal: proposal_pda,
            transaction: transaction_pda,
            member: member_1.pubkey(), // executor
        },
        &[], // alt's
        None,
    )
    .await
    .expect("failed to build execute ix");

    let tx = Transaction::new(
        &[member_1.insecure_clone()],
        Message::new(&[proposal_execute_ix], Some(&member_1.pubkey())),
        svm.latest_blockhash(),
    );

    println!(
        "Proposal Executed Logs:\n{:#?}",
        svm.send_transaction(tx).unwrap().logs
    );

    // Verify funds reached receiver
    let receiver_balance = svm.get_balance(&receiver.pubkey()).unwrap();
    println!("Receiver balance after execution: {}", receiver_balance);

    // ------------------------------------------------------------------------------
    // 7. Close Proposal & Transaction Accounts (cleanup)
    // ------------------------------------------------------------------------------
    let close_ix = proposal_accounts_close(
        ProposalAccountsCloseAccounts {
            multisig: multisig_pda,
            proposal: proposal_pda,
            transaction: transaction_pda,
            rent_collector: creator.pubkey(),
            system_program: SYSTEM_PROGRAM_ID,
        },
        None,
    );

    let tx = Transaction::new(
        &[creator.insecure_clone()],
        Message::new(&[close_ix], Some(&creator.pubkey())),
        svm.latest_blockhash(),
    );

    println!(
        "Proposal Accounts Close Logs:\n{:#?}",
        svm.send_transaction(tx).unwrap().logs
    );
}
