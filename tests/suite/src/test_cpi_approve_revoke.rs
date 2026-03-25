use {
    crate::helpers::*,
    quasar_svm::{Account, Instruction, Pubkey},
    quasar_test_token_cpi::client::*,
};

// ===========================================================================
// Approve (discriminator 1) — Program<Token>
// ===========================================================================

#[test]
fn approve_spl() {
    let mut svm = svm_cpi();
    let authority = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let source_key = Pubkey::new_unique();
    let delegate_key = Pubkey::new_unique();
    let token_program = spl_token_program_id();

    let instruction: Instruction = ApproveInstruction {
        authority,
        source: source_key,
        delegate: delegate_key,
        token_program,
        amount: 500,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (authority, signer_account()),
            (source_key, token_account(mint_key, authority, 1000, token_program)),
            (delegate_key, signer_account()),
        ],
    );
    assert!(result.is_ok(), "approve SPL should succeed: {:?}", result.raw_result);
}

// ===========================================================================
// ApproveT22 (discriminator 22) — Program<Token2022>
// ===========================================================================

#[test]
fn approve_t22() {
    let mut svm = svm_cpi();
    let authority = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let source_key = Pubkey::new_unique();
    let delegate_key = Pubkey::new_unique();
    let token_program = token_2022_program_id();

    let instruction: Instruction = ApproveT22Instruction {
        authority,
        source: source_key,
        delegate: delegate_key,
        token_program,
        amount: 500,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (authority, signer_account()),
            (source_key, token_account(mint_key, authority, 1000, token_program)),
            (delegate_key, signer_account()),
        ],
    );
    assert!(result.is_ok(), "approve T22 should succeed: {:?}", result.raw_result);
}

// ===========================================================================
// ApproveInterface (discriminator 23) — Interface<TokenInterface>
// ===========================================================================

#[test]
fn approve_interface_spl() {
    let mut svm = svm_cpi();
    let authority = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let source_key = Pubkey::new_unique();
    let delegate_key = Pubkey::new_unique();
    let token_program = spl_token_program_id();

    let instruction: Instruction = ApproveInterfaceInstruction {
        authority,
        source: source_key,
        delegate: delegate_key,
        token_program,
        amount: 500,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (authority, signer_account()),
            (source_key, token_account(mint_key, authority, 1000, token_program)),
            (delegate_key, signer_account()),
        ],
    );
    assert!(
        result.is_ok(),
        "approve interface SPL should succeed: {:?}",
        result.raw_result
    );
}

#[test]
fn approve_interface_t22() {
    let mut svm = svm_cpi();
    let authority = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let source_key = Pubkey::new_unique();
    let delegate_key = Pubkey::new_unique();
    let token_program = token_2022_program_id();

    let instruction: Instruction = ApproveInterfaceInstruction {
        authority,
        source: source_key,
        delegate: delegate_key,
        token_program,
        amount: 500,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (authority, signer_account()),
            (source_key, token_account(mint_key, authority, 1000, token_program)),
            (delegate_key, signer_account()),
        ],
    );
    assert!(
        result.is_ok(),
        "approve interface T22 should succeed: {:?}",
        result.raw_result
    );
}

// ===========================================================================
// Revoke (discriminator 2) — Program<Token>
// ===========================================================================

#[test]
fn revoke_spl() {
    let mut svm = svm_cpi();
    let authority = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let source_key = Pubkey::new_unique();
    let delegate_key = Pubkey::new_unique();
    let token_program = spl_token_program_id();

    let instruction: Instruction = RevokeInstruction {
        authority,
        source: source_key,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (authority, signer_account()),
            (source_key, Account {
                lamports: 1_000_000,
                data: pack_token_account_with_delegate(mint_key, authority, 1000, delegate_key, 500),
                owner: token_program,
                executable: false,
                rent_epoch: 0,
            }),
        ],
    );
    assert!(result.is_ok(), "revoke SPL should succeed: {:?}", result.raw_result);
}

// ===========================================================================
// RevokeT22 (discriminator 24) — Program<Token2022>
// ===========================================================================

#[test]
fn revoke_t22() {
    let mut svm = svm_cpi();
    let authority = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let source_key = Pubkey::new_unique();
    let delegate_key = Pubkey::new_unique();
    let token_program = token_2022_program_id();

    let instruction: Instruction = RevokeT22Instruction {
        authority,
        source: source_key,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (authority, signer_account()),
            (source_key, Account {
                lamports: 1_000_000,
                data: pack_token_account_with_delegate(mint_key, authority, 1000, delegate_key, 500),
                owner: token_program,
                executable: false,
                rent_epoch: 0,
            }),
        ],
    );
    assert!(result.is_ok(), "revoke T22 should succeed: {:?}", result.raw_result);
}

// ===========================================================================
// RevokeInterface (discriminator 25) — Interface<TokenInterface>
// ===========================================================================

#[test]
fn revoke_interface_spl() {
    let mut svm = svm_cpi();
    let authority = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let source_key = Pubkey::new_unique();
    let delegate_key = Pubkey::new_unique();
    let token_program = spl_token_program_id();

    let instruction: Instruction = RevokeInterfaceInstruction {
        authority,
        source: source_key,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (authority, signer_account()),
            (source_key, Account {
                lamports: 1_000_000,
                data: pack_token_account_with_delegate(mint_key, authority, 1000, delegate_key, 500),
                owner: token_program,
                executable: false,
                rent_epoch: 0,
            }),
        ],
    );
    assert!(
        result.is_ok(),
        "revoke interface SPL should succeed: {:?}",
        result.raw_result
    );
}

#[test]
fn revoke_interface_t22() {
    let mut svm = svm_cpi();
    let authority = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let source_key = Pubkey::new_unique();
    let delegate_key = Pubkey::new_unique();
    let token_program = token_2022_program_id();

    let instruction: Instruction = RevokeInterfaceInstruction {
        authority,
        source: source_key,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (authority, signer_account()),
            (source_key, Account {
                lamports: 1_000_000,
                data: pack_token_account_with_delegate(mint_key, authority, 1000, delegate_key, 500),
                owner: token_program,
                executable: false,
                rent_epoch: 0,
            }),
        ],
    );
    assert!(
        result.is_ok(),
        "revoke interface T22 should succeed: {:?}",
        result.raw_result
    );
}
