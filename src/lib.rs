#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "defmt")]
pub fn only_on_defmt() {}
