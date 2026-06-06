use std::io::Write;

use shocking_vrc_core::cli::UkfConfig;

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
