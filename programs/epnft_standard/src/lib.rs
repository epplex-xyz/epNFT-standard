pub use anchor_lang::{
    prelude::*,
    system_program::{ID as SYSTEM_PROGRAM_ID, create_account, CreateAccount},
    solana_program::{
        sysvar::instructions::{ID as INSTRUCTIONS_ID, load_current_index_checked, load_instruction_at_checked},
        system_program::ID as SOLANA_SYSTEM_PROGRAM_ID,
    },
};
pub use anchor_spl::{
    token_2022::ID as TOKEN_2022_PROGRAM_ID,
    associated_token::{AssociatedToken, ID as ASSOCIATED_TOKEN_PROGRAM_ID},
    token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked},
};
pub use spl_tlv_account_resolution::{
    account::ExtraAccountMeta, seeds::Seed, state::ExtraAccountMetaList,
};
pub use spl_transfer_hook_interface::instruction::{ExecuteInstruction, TransferHookInstruction};

declare_id!("GwUqKeSYPfuGq8YAKHNfEKTEfX3rfEz8ygLgGeVBLz8a");

#[program]
pub mod epnft_standard {
    use super::*;

    pub fn initialize_extra_account_meta_list(
        ctx: Context<InitializeExtraAccountMetaList>,
    ) -> Result<()> {
        // index 0-3 are the accounts required for token transfer (source, mint, destination, owner)
        // index 4 is address of ExtraAccountMetaList account
        let account_metas = vec![
            // index 5, sysvar_instruction
            ExtraAccountMeta::new_with_pubkey(&ctx.accounts.sysvar_instruction.key(), false, false)?,
        ];

        // calculate account size
        let account_size = ExtraAccountMetaList::size_of(account_metas.len())? as u64;
        // calculate minimum required lamports
        let lamports = Rent::get()?.minimum_balance(account_size as usize);

        let mint = ctx.accounts.mint.key();
        let signer_seeds: &[&[&[u8]]] = &[&[
            b"extra-account-metas",
            &mint.as_ref(),
            &[ctx.bumps.extra_account_meta_list],
        ]];

        // create ExtraAccountMetaList account
        create_account(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                CreateAccount {
                    from: ctx.accounts.payer.to_account_info(),
                    to: ctx.accounts.extra_account_meta_list.to_account_info(),
                },
            )
            .with_signer(signer_seeds),
            lamports,
            account_size,
            ctx.program_id,
        )?;

        // initialize ExtraAccountMetaList account with extra accounts
        ExtraAccountMetaList::init::<ExecuteInstruction>(
            &mut ctx.accounts.extra_account_meta_list.try_borrow_mut_data()?,
            &account_metas,
        )?;

        Ok(())
    }

    // If this fails, the initial token transfer fails
    pub fn transfer_hook(ctx: Context<TransferHook>, amount: u64) -> Result<()> {

        /* Our Ruleset is powered by Instruction Introspection */

        let ixs = ctx.accounts.sysvar_instruction.to_account_info();

        /*
        
            Custom Rule 1#:

            Objective: This rule kills all the possibility of bypassing the ruleset by just 
            having to check the instruction after this one in a deterministic way!

            Possibility:
            - This instruction is in position 0. Wonderful!
            - This instruction is in position 1. We need to check that the one in position 0 
            is actually the creation of the Destination Token (or ATA).
            - This instruction is in position 2 or more. GTFO!
        
        */

        let current_index = load_current_index_checked(&ixs)? as usize;

        match current_index {
            0 => { 
                // Do nothing 
            }
            1 => {
                // Check the Ix at position 0
                let ix = load_instruction_at_checked(current_index - 1, &ixs)?;

                // Check that is a Create Account instruction. We don't need more because at this point we're covering more than enough edge cases.
                require_keys_eq!(ix.program_id, ASSOCIATED_TOKEN_PROGRAM_ID, TransferHookErr::InvalidProgram1);
                require_eq!(ix.data[0], 1u8, TransferHookErr::InvalidIx1);
            },
            _ => {
                // We don't want this instruction in any other position
                return Err(TransferHookErr::InvalidInstructionPosition.into());
            }
        }
        
        /*

            Custom Rule 2#:

            Objective: This rule kills all unwanted CPI by checking the ID of the program that is calling 
            this instruction and verifying that it's the one associated with the TransferHook Program.
            This is needed to close off the possibility of bypassing our ruleset by operating with an outside 
            program.

            Possibility:
            - The program is the one we want. Wonderful!
            - The program is on our CPI Allowlist. Wonderful!
            - The program is not the one we want, and it's not on our CPI Allowlist. GTFO!
        
            Example:
            >> Program we want to allow to CPI from
            if ix.program_id == PROGRAM_ID {
                >> Discriminator from that program we want to allow
                if ix.data[0..8] == DISCRIMINATOR 1 {
                    // Other Logic
                >> We can check for multiple Discriminator with different Logic
                } else if ix.data[0..8] == DISCRIMINATOR 2 {
                    // Other Logic
                } else {
                    >> We want to stop CPI from Allowlist Program but from other Instruction
                    return Err(TransferHookErr::UnauthorizedInstruction2.into());
                }
            } else {
                >> We don't want any program that are not on our Allowlist
                return Err(TransferHookErr::UnauthorizedCpi.into());
            }

        */

        let ix = load_instruction_at_checked(current_index, &ixs)?;

        if ix.program_id == TOKEN_2022_PROGRAM_ID {
            // Do nothing
        } else if ix.program_id == burger_marketplace_program_id::ID {
            if ix.data[0..8] == LIST_INSTRUCTION_BURGER_MARKETPLACE || ix.data[0..8] == DELIST_INSTRUCTION_BURGER_MARKETPLACE{
                return Ok(());
            } else if ix.data[0..8] == BUY_INSTRUCTION_BURGER_MARKETPLACE {
                // We find the amount that the User is paying for the NFT
                let buy_amount = u64::from_le_bytes(ix.data[8..16].try_into().unwrap());

                // We're making sure that they're paying royalties after
                let ix = load_instruction_at_checked(current_index + 1, &ixs)?;
                require_keys_eq!(ix.program_id, SOLANA_SYSTEM_PROGRAM_ID, TransferHookErr::TBD);
                require_eq!(ix.data[0], 2u8, TransferHookErr::TBD);
                require!(ix.data[4..12].eq(&buy_amount.checked_div(100).unwrap().to_le_bytes()), TransferHookErr::TBD); // 1% Royalties - For Testing Purpose
                require_keys_eq!(ix.accounts.get(1).unwrap().pubkey, Pubkey::default(), TransferHookErr::TBD);  // The Creator of the NFT - For Testing Purpose

                return Ok(());
            } else {
                return Err(TransferHookErr::UnauthorizedInstruction2.into());
            }

        } else {
            return Err(TransferHookErr::UnauthorizedCpi.into());
        }
    
        /*
        
            Custom Rule 3#:

            Objective: This rule check for all the instruction after this one to make sure that
            they're actually interacting with program that are on our program Allowlist.

            Possibility:
            - There is no instruction after this one. Wonderful! -> Edge case of a simple transfer
            - The instruction after this one is executed by a program that is on our Allowlist. Wonderful!
            - The instruction after this one is executed by a program that is not on our Allowlist. GTFO!

            Example:
            >> Program we want to allow
            if ix.program_id == SYSTEM_PROGRAM_ID {
                >> Discriminator we want to allow
                if ix.data[0] == 2u8 {
                    // Other Logic
                >> Can be Multiple with Different Logic
                } else if ix.data[0] == 1u8 {
                    // Other Logic
                } else {
                    >> We don't want any Discriminator we don't know about
                    return Err(TransferHookErr::UnauthorizedTrailingInstruction.into());
                }
                >> Stop other Instruction we don't need
                require_neq!(load_instruction_at_checked(current_index + 2, &ixs).is_ok(), true, TransferHookErr::UnauthorizedTrailingInstruction);
            } else {
                >> We don't want any instruction we don't know about
                return Err(TransferHookErr::UnauthorizedTrailingProgram.into());
            }

        */

        if let Ok(ix) = load_instruction_at_checked(current_index + 1, &ixs) {
            return Err(TransferHookErr::UnauthorizedTrailingProgram.into());
        }
        
        Ok(())
    }

    // fallback instruction handler as workaround to anchor instruction discriminator check
    pub fn fallback<'info>(
        program_id: &Pubkey,
        accounts: &'info [AccountInfo<'info>],
        data: &[u8],
    ) -> Result<()> {
        let instruction = TransferHookInstruction::unpack(data)?;

        // match instruction discriminator to transfer hook interface execute instruction
        // token2022 program CPIs this instruction on token transfer
        match instruction {
            TransferHookInstruction::Execute { amount } => {
                let amount_bytes = amount.to_le_bytes();

                // invoke custom transfer hook instruction on our program
                __private::__global::transfer_hook(program_id, accounts, &amount_bytes)
            }
            _ => return Err(ProgramError::InvalidInstructionData.into()),
        }
    }
}

#[derive(Accounts)]
pub struct InitializeExtraAccountMetaList<'info> {
    #[account(mut)]
    payer: Signer<'info>,

    /// CHECK: ExtraAccountMetaList Account, must use these seeds
    #[account(
        mut,
        seeds = [b"extra-account-metas", mint.key().as_ref()],
        bump
    )]
    pub extra_account_meta_list: AccountInfo<'info>,
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(address = INSTRUCTIONS_ID)]
    /// CHECK: Sysvar instruction account
    pub sysvar_instruction: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

// Order of accounts matters for this struct.
// The first 4 accounts are the accounts required for token transfer (source, mint, destination, owner)
// Remaining accounts are the extra accounts required from the ExtraAccountMetaList account
// These accounts are provided via CPI to this program from the token2022 program
#[derive(Accounts)]
pub struct TransferHook<'info> {
    #[account(
        token::mint = mint,
        token::authority = owner,
    )]
    pub source_token: InterfaceAccount<'info, TokenAccount>,
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(
        token::mint = mint,
    )]
    pub destination_token: InterfaceAccount<'info, TokenAccount>,
    /// CHECK: source token account owner, can be SystemAccount or PDA owned by another program
    pub owner: UncheckedAccount<'info>,
    /// CHECK: ExtraAccountMetaList Account,
    #[account(
        seeds = [b"extra-account-metas", mint.key().as_ref()],
        bump
    )]
    pub extra_account_meta_list: UncheckedAccount<'info>,
    #[account(address = INSTRUCTIONS_ID)]
    /// CHECK: Sysvar instruction account
    pub sysvar_instruction: AccountInfo<'info>,
}

#[error_code]
pub enum TransferHookErr {

    // Rule 1#
    #[msg("The TransferChecked instruction needs to be in Position 0")]
    InvalidInstructionPosition,
    #[msg("Rule 1# - The Program ID is not Authorized")]
    InvalidProgram1,
    #[msg("Rule 1# - The Instruction is not Authorized")]
    InvalidIx1,

    // Rule 2#
    #[msg("You're transferring this token using a CPI from an unauthorized program")]
    UnauthorizedCpi,
    #[msg("Rule 2# - The Instruction is not Authorized")]
    UnauthorizedInstruction2,
    #[msg("Rule 2# - TBD")]
    TBD,

    // Rule 3#
    #[msg("You're using an unauthorized instruction after the transfer instruction")]
    UnauthorizedTrailingInstruction,
    #[msg("You're using an unauthorized program after the transfer instruction")]
    UnauthorizedTrailingProgram,
    #[msg("Rule 3# - The Instruction is not Authorized")]
    InvalidIx3,
    #[msg("Rule 3# - The Amount is not the correct one")]
    InvalidAmount3,
}

// Constant for Ruleset Check
pub mod burger_marketplace_program_id {
    use super::*;
    declare_id!("AhC8ej2B8LYF86ic16ZFZ4EGAxgcNz7Hvbx1pYdiAHqm");
}

pub const LIST_INSTRUCTION_BURGER_MARKETPLACE: [u8; 8] = [244, 251, 143, 66, 248, 70, 67, 211];
pub const DELIST_INSTRUCTION_BURGER_MARKETPLACE: [u8; 8] = [184, 61, 232, 55, 238, 38, 20, 149];
pub const BUY_INSTRUCTION_BURGER_MARKETPLACE: [u8; 8] = [7, 139, 71, 153, 193, 172, 127, 137];