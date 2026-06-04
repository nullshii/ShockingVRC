use std::io::Write;

use shocking_vrc_core::cli::{MotionNorms, UkfConfig};

use crate::engine::command::{Command, CommandData, CommandFuture};

pub struct UkfCommand;

impl Command for UkfCommand {
    fn names(&self) -> &[&str] {
        &["ukf"]
    }

    fn description(&self) -> &str {
        "View/set UKF filter parameters. Usage: ukf [q r [alpha beta kappa] | reset]"
    }

    fn execute(&self, _cmd_name: String, args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;

            match args.as_slice() {
                [] => {
                    let p = data.state.engine.ukf_params().await;
                    writeln!(
                        w,
                        "[ukf] q={:.4}  r={:.4}  alpha={:.3}  beta={:.3}  kappa={:.3}",
                        p.q, p.r, p.alpha, p.beta, p.kappa
                    )?;
                }
                [sub] if matches!(sub.as_str(), "reset" | "default" | "defaults") => {
                    data.state.engine.set_ukf_params(UkfConfig::default()).await;
                    writeln!(w, "[ukf] Reset to defaults")?;
                }
                [q, r] => match (q.parse::<f32>(), r.parse::<f32>()) {
                    (Ok(qv), Ok(rv)) if qv > 0.0 && rv > 0.0 => {
                        let mut p = data.state.engine.ukf_params().await;
                        p.q = qv;
                        p.r = rv;
                        data.state.engine.set_ukf_params(p).await;
                        writeln!(w, "[ukf] q={qv:.4}  r={rv:.4}")?;
                    }
                    _ => writeln!(
                        w,
                        "Usage: ukf <q> <r>  (positive floats; q=process noise, r=measurement noise)"
                    )?,
                },
                [q, r, alpha, beta, kappa] => {
                    match (
                        q.parse::<f32>(),
                        r.parse::<f32>(),
                        alpha.parse::<f32>(),
                        beta.parse::<f32>(),
                        kappa.parse::<f32>(),
                    ) {
                        (Ok(qv), Ok(rv), Ok(av), Ok(bv), Ok(kv))
                            if qv > 0.0 && rv > 0.0 && av > 0.0 =>
                        {
                            data.state
                                .engine
                                .set_ukf_params(UkfConfig {
                                    q: qv,
                                    r: rv,
                                    alpha: av,
                                    beta: bv,
                                    kappa: kv,
                                })
                                .await;
                            writeln!(
                                w,
                                "[ukf] q={qv:.4}  r={rv:.4}  alpha={av:.3}  beta={bv:.3}  kappa={kv:.3}"
                            )?;
                        }
                        _ => writeln!(
                            w,
                            "Usage: ukf <q> <r> <alpha> <beta> <kappa>  (q,r,alpha > 0; typical alpha 0.5, beta 2, kappa 0)"
                        )?,
                    }
                }
                _ => writeln!(
                    w,
                    "Usage: ukf [q r [alpha beta kappa] | reset]"
                )?,
            }
            Ok(())
        })
    }
}

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
