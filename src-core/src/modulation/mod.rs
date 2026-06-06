pub mod config;
pub mod evaluator;
pub mod functions;
pub mod noise;

pub use config::{ModulationConfig, ModulationFunction, ModulationSource};
pub use evaluator::{KinematicsInput, advance_and_evaluate_segments, evaluate_with_accumulator};
