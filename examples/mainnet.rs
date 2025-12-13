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
use solana_sdk::{
    message::Message,
    pubkey::Pubkey,
    signature::Keypair,
    signer::{EncodableKey, Signer},
    transaction::Transaction,
};
use solana_system_interface::instruction::transfer as native_transfer;

pub const SYSTEM_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("11111111111111111111111111111111");
use solana_client::nonblocking::rpc_client::RpcClient;
#[tokio::main]
pub async fn main() {
    let kp_path = "PATH_TO_WALLET";
    let cluster = "https://api.mainnet-beta.solana.com".to_string();
    let rpc = RpcClient::new(cluster);

    let bob = Keypair::read_from_file(kp_path).unwrap();
    let alice = Keypair::new();

    let threshold = 1;
    let members = vec![bob.pubkey(), alice.pubkey()];

    let create_key = Keypair::new();
    let multisig_pda = get_multisig_pda(&create_key.pubkey(), None).0;
    let vault_pda = get_vault_pda(&multisig_pda, None).0;
    let multisig_create_ix = multisig_create(
        MultisigCreateAccounts {
            treasury: TREASURY,
            multisig: multisig_pda,
            create_key: create_key.pubkey(),
            creator: bob.pubkey(),
            system_program: SYSTEM_PROGRAM_ID,
        },
        MultisigCreateArgs {
            threshold,
            rent_collector: Some(bob.pubkey()),
            members,
        },
        None,
    );
    let transfer_to_vault_ix = native_transfer(&bob.pubkey(), &vault_pda, 1_000_000);
    println!("Fortis program id: {}", multisig_create_ix.program_id);

    let transaction_index = 1u64;

    let transaction_pda = get_transaction_pda(&multisig_pda, transaction_index, None).0;
    let proposal_pda = get_proposal_pda(&multisig_pda, transaction_index, None).0;

    let proposal_accounts = ProposalCreateAccounts {
        multisig: multisig_pda,
        trasaction: transaction_pda,
        creator: bob.pubkey(),
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
        &[bob.insecure_clone(), create_key.insecure_clone()],
        Message::new(
            &[multisig_create_ix, transfer_to_vault_ix, proposal_create_ix],
            Some(&bob.pubkey()),
        ),
        rpc.get_latest_blockhash().await.unwrap(),
    );
    println!(
        "Transaction 1:\n{:#?}",
        rpc.send_and_confirm_transaction(&tx).await
    );

    let approve_ix = proposal_approve(
        ProposalApproveAccounts {
            multisig: multisig_pda,
            proposal: proposal_pda,
            member: bob.pubkey(),
        },
        ProposalApproveArgs {},
        None,
    );
    let proposal_execute_ix = proposal_execute(
        &rpc.get_account_data(&transaction_pda)
            .await
            .expect("transaction PDA missing"),
        ProposalExecuteAccounts {
            multisig: multisig_pda,
            proposal: proposal_pda,
            transaction: transaction_pda,
            member: bob.pubkey(), // executor
        },
        &[], // alt's
        None,
    )
    .await
    .expect("failed to build execute ix");

    let tx = Transaction::new(
        &[bob.insecure_clone()],
        Message::new(&[approve_ix, proposal_execute_ix], Some(&bob.pubkey())),
        rpc.get_latest_blockhash().await.unwrap(),
    );

    println!(
        "Transaction 2 :\n{:#?}",
        rpc.send_and_confirm_transaction(&tx).await
    );
    let close_ix = proposal_accounts_close(
        ProposalAccountsCloseAccounts {
            multisig: multisig_pda,
            proposal: proposal_pda,
            transaction: transaction_pda,
            rent_collector: bob.pubkey(),
            system_program: SYSTEM_PROGRAM_ID,
        },
        None,
    );

    let tx = Transaction::new(
        &[bob.insecure_clone()],
        Message::new(&[close_ix], Some(&bob.pubkey())),
        rpc.get_latest_blockhash().await.unwrap(),
    );

    println!(
        "Proposal Accounts Close:\n{:#?}",
        rpc.send_and_confirm_transaction(&tx).await
    );
}
