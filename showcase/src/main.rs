#![forbid(unsafe_code)]

use nice_plug::prelude::*;
use tinyviolin_showcase::TinyViolinShowcase;

fn main() {
    nice_export_standalone::<TinyViolinShowcase>();
}
