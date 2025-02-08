use probe_rs_target::{
    Chip,
};

use crate::{
    config::DebugSequence,
    vendor::{
        Vendor,
        rp::sequences::rp2040::Rp2040,
    },
};

pub mod sequences;

/// Raspberry Pi
#[derive(docsplay::Display)]
pub struct Rp;

impl Vendor for Rp {
    fn try_create_debug_sequence(&self, chip: &Chip) -> Option<DebugSequence> {
        Some(
            if chip.name == "RP2040" {
                // exact match to not include RP2040_SELFDEBUG
                // since in that case we really want to reset core0 only,
                // not both (which is what the sequence does).
                tracing::warn!("Using custom sequence for RP2040");
                DebugSequence::Arm(Rp2040::create())
            } else {
                return None
            }
        )
    }

    // TODO? copy in/factor ../nordicsemi/mod.rs's try_detect_arm_chip()?
}
