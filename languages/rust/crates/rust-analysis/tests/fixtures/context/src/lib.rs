//! The context workspace (RS4 §8): the programs layer 2 (RC1 to RC5) is about, written now with their layer-1 trees and `open_*` counts as snapshots, so no
//! RC phase writes a program under pressure. Each phase changes some of those snapshots on purpose. Layer 1 knows one context per function: a generic, a
//! callback and a `dyn` are each one node whose callee is the trait's declaration or an unresolved call, and `open_*` counts how many such answers there are.
#![allow(dead_code, unused)]

mod callback;
mod dyn_local;
mod dyn_param;
mod factory;
mod generic;
mod holder;
mod recursive;

pub trait Writer {
    fn write(&self) -> u8;
}

pub struct Json;

pub struct Csv;

impl Writer for Json {
    fn write(&self) -> u8 {
        1
    }
}

impl Writer for Csv {
    fn write(&self) -> u8 {
        2
    }
}
