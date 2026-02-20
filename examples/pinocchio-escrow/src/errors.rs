use pinocchio::error::ProgramError;

#[repr(u32)]
pub enum EscrowError {
    InvalidInstructionData = 0,
    NotEnoughAccountKeys = 1,
    MissingRequiredSignature = 2,
    InvalidAccountOwner = 3,
    InvalidPDA = 4,
    InvalidEscrowState = 5,
    InvalidMaker = 6,
    InvalidMakerTokenAccount = 7,
    ZeroReceiveAmount = 8,
    IncorrectTokenProgram = 9,
    IncorrectSystemProgram = 10,
}

impl From<EscrowError> for ProgramError {
    fn from(e: EscrowError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
