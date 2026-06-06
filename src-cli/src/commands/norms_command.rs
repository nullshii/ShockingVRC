use std::io::Write;

use shocking_vrc_core::cli::MotionNorms;

use crate::engine::command::{Command, CommandData, CommandFuture};

pub struct NormsCommand;

impl Command for NormsCommand {
    fn names(&self) -> &[&str] {
        &["norms", "norm-speed", "norm-acc", "norm-recoil"]
    }

    fn description(&self) -> &str {
        "View/set motion normalisation divisors. Usage: norms [speed acc recoil | reset]"
    }

    fn execute(&self, cmd_name: String, args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;

            match cmd_name.as_str() {
                "norms" => match args.as_slice() {
                    [] => {
                        let n = data.state.engine.norms().await;
                        writeln!(
                            w,
                            "[norms] speed={:.3}  acc={:.3}  recoil={:.3}",
                            n.speed, n.acc, n.recoil
                        )?;
                    }
                    [sub] if matches!(sub.as_str(), "reset" | "default" | "defaults") => {
                        data.state.engine.set_norms(MotionNorms::default()).await;
                        let n = data.state.engine.norms().await;
                        writeln!(
                            w,
                            "[norms] Reset to defaults: speed={:.3}  acc={:.3}  recoil={:.3}",
                            n.speed, n.acc, n.recoil
                        )?;
                    }
                    [speed, acc, recoil] => {
                        match (speed.parse::<f32>(), acc.parse::<f32>(), recoil.parse::<f32>()) {
                            (Ok(sv), Ok(av), Ok(rv)) if sv > 0.0 && av > 0.0 && rv > 0.0 => {
                                data.state
                                    .engine
                                    .set_norms(MotionNorms { speed: sv, acc: av, recoil: rv })
                                    .await;
                                writeln!(w, "[norms] speed={sv:.3}  acc={av:.3}  recoil={rv:.3}")?;
                            }
                            _ => writeln!(
                                w,
                                "Usage: norms <speed> <acc> <recoil>  (all positive floats)"
                            )?,
                        }
                    }
                    _ => writeln!(w, "Usage: norms [speed acc recoil | reset]")?,
                },

                "norm-speed" => match args.first().and_then(|v| v.parse::<f32>().ok()) {
                    Some(val) if val > 0.0 => {
                        let mut n = data.state.engine.norms().await;
                        n.speed = val;
                        data.state.engine.set_norms(n).await;
                        writeln!(w, "[norms] speed={val:.3}")?;
                    }
                    _ => writeln!(w, "Usage: norm-speed <positive float>")?,
                },

                "norm-acc" => match args.first().and_then(|v| v.parse::<f32>().ok()) {
                    Some(val) if val > 0.0 => {
                        let mut n = data.state.engine.norms().await;
                        n.acc = val;
                        data.state.engine.set_norms(n).await;
                        writeln!(w, "[norms] acc={val:.3}")?;
                    }
                    _ => writeln!(w, "Usage: norm-acc <positive float>")?,
                },

                "norm-recoil" => match args.first().and_then(|v| v.parse::<f32>().ok()) {
                    Some(val) if val > 0.0 => {
                        let mut n = data.state.engine.norms().await;
                        n.recoil = val;
                        data.state.engine.set_norms(n).await;
                        writeln!(w, "[norms] recoil={val:.3}")?;
                    }
                    _ => writeln!(w, "Usage: norm-recoil <positive float>")?,
                },

                _ => {}
            }
            Ok(())
        })
    }
}
