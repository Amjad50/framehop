use gimli::{
    CfaRule, Encoding, EvaluationStorage, Reader, Register, RegisterRule, UnwindContextStorage,
    UnwindTableRow, X86_64,
};

use super::{arch::ArchX86_64, unwind_rule::UnwindRuleX86_64, unwindregs::UnwindRegsX86_64};
use crate::dwarf::{
    eval_cfa_rule, eval_register_rule, ConversionError, DwarfUnwindRegs, DwarfUnwinderError,
    DwarfUnwinding,
};
use crate::unwind_result::UnwindResult;
use crate::FrameAddress;

impl DwarfUnwindRegs for UnwindRegsX86_64 {
    fn get(&self, register: Register) -> Option<u64> {
        match register {
            X86_64::RA => Some(self.ip()),
            X86_64::RSP => Some(self.sp()),
            X86_64::RBP => Some(self.bp()),
            _ => None,
        }
    }
}

impl DwarfUnwinding for ArchX86_64 {
    fn unwind_frame<F, R, S>(
        unwind_info: &UnwindTableRow<R, S>,
        encoding: Encoding,
        regs: &mut Self::UnwindRegs,
        address: FrameAddress,
        read_mem: &mut F,
    ) -> Result<UnwindResult<Self::UnwindRule>, DwarfUnwinderError>
    where
        F: FnMut(u64) -> Result<u64, ()>,
        R: Reader,
        S: UnwindContextStorage<R> + EvaluationStorage<R>,
    {
        let cfa_rule = unwind_info.cfa();
        let bp_rule = unwind_info.register(X86_64::RBP);
        let ra_rule = unwind_info.register(X86_64::RA);

        match translate_into_unwind_rule(cfa_rule, &bp_rule, &ra_rule) {
            Ok(unwind_rule) => return Ok(UnwindResult::ExecRule(unwind_rule)),
            Err(err) => {
                eprintln!("Unwind rule translation failed: {:?}", err);
            }
        }

        let cfa = eval_cfa_rule::<R, _, S>(cfa_rule, encoding, regs)
            .ok_or(DwarfUnwinderError::CouldNotRecoverCfa)?;

        let ip = regs.ip();
        let bp = regs.bp();
        let sp = regs.sp();

        let new_bp = eval_register_rule::<R, F, _, S>(bp_rule, cfa, encoding, bp, regs, read_mem)
            .unwrap_or(bp);

        let return_address =
            match eval_register_rule::<R, F, _, S>(ra_rule, cfa, encoding, ip, regs, read_mem) {
                Some(ra) => ra,
                None => read_mem(cfa - 8)
                    .map_err(|_| DwarfUnwinderError::CouldNotRecoverReturnAddress)?,
            };

        if cfa == sp && return_address == ip {
            return Err(DwarfUnwinderError::DidNotAdvance);
        }
        if address.is_return_address() && cfa < regs.sp() {
            return Err(DwarfUnwinderError::StackPointerMovedBackwards);
        }

        regs.set_ip(return_address);
        regs.set_bp(new_bp);
        regs.set_sp(cfa);

        Ok(UnwindResult::Uncacheable(return_address))
    }
}

fn register_rule_to_cfa_offset<R: gimli::Reader>(
    rule: &RegisterRule<R>,
) -> Result<Option<i64>, ConversionError> {
    match *rule {
        RegisterRule::Undefined | RegisterRule::SameValue => Ok(None),
        RegisterRule::Offset(offset) => Ok(Some(offset)),
        _ => Err(ConversionError::RegisterNotStoredRelativeToCfa),
    }
}

fn translate_into_unwind_rule<R: gimli::Reader>(
    cfa_rule: &CfaRule<R>,
    bp_rule: &RegisterRule<R>,
    ra_rule: &RegisterRule<R>,
) -> Result<UnwindRuleX86_64, ConversionError> {
    match ra_rule {
        RegisterRule::Undefined => {
            // This is normal. Return address is [CFA-8].
        }
        RegisterRule::Offset(offset) => {
            if *offset == -8 {
                // Weirdly explicit, but also ok.
            } else {
                // Not ok.
                return Err(ConversionError::ReturnAddressRuleWithUnexpectedOffset);
            }
        }
        _ => {
            // Somebody's being extra. Go down the slow path.
            return Err(ConversionError::ReturnAddressRuleWasWeird);
        }
    }

    match cfa_rule {
        CfaRule::RegisterAndOffset { register, offset } => match *register {
            X86_64::RSP => {
                let sp_offset_by_8 =
                    u16::try_from(offset / 8).map_err(|_| ConversionError::SpOffsetDoesNotFit)?;
                let fp_cfa_offset = register_rule_to_cfa_offset(bp_rule)?;
                match fp_cfa_offset {
                    None => Ok(UnwindRuleX86_64::OffsetSp { sp_offset_by_8 }),
                    Some(bp_cfa_offset) => {
                        let bp_storage_offset_from_sp_by_8 =
                            i8::try_from((offset + bp_cfa_offset) / 8)
                                .map_err(|_| ConversionError::FpStorageOffsetDoesNotFit)?;
                        Ok(UnwindRuleX86_64::OffsetSpAndRestoreBp {
                            sp_offset_by_8,
                            bp_storage_offset_from_sp_by_8,
                        })
                    }
                }
            }
            X86_64::RBP => {
                let bp_cfa_offset = register_rule_to_cfa_offset(bp_rule)?
                    .ok_or(ConversionError::FramePointerRuleDoesNotRestoreBp)?;
                if *offset == 16 && bp_cfa_offset == -16 {
                    Ok(UnwindRuleX86_64::UseFramePointer)
                } else {
                    Err(ConversionError::FramePointerRuleHasStrangeBpOffset)
                }
            }
            _ => Err(ConversionError::CfaIsOffsetFromUnknownRegister),
        },
        CfaRule::Expression(_) => Err(ConversionError::CfaIsExpression),
    }
}
