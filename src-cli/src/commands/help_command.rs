use std::io::Write;

use crate::engine::command::{Command, CommandData, CommandFuture};

pub struct HelpCommand;

impl Command for HelpCommand {
    fn names(&self) -> &[&str] {
        &["help", "h", "?"]
    }

    fn description(&self) -> &str {
        "Print list of commands."
    }

    fn execute(&self, _cmd_name: String, _args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            writeln!(w, "
Zone commands  (type: Orf | Pen | Touch | DGB  |  * = wildcard)
  add-a  <type> <name> [mode]       Add exact zone to channel A (mode: depth|speed|acc|recoil, default depth)
  add-a  Orf * [mode]               Add ALL Orf zones to channel A (wildcard)
  add-a  * * [mode]                 Add every avatar zone to channel A
  add-b  <type> <name | *> [mode]   Same for channel B
  mode-a <type> <name | *> <mode>   Change mode of an existing entry on A
  mode-b <type> <name | *> <mode>   Change mode of an existing entry on B
  add-all-a [type]                  Add all avatar zones currently seen to A
  add-all-b [type]                  Add all avatar zones currently seen to B
  rm-a   <type> <name | *>          Remove zone/pattern from channel A
  rm-b   <type> <name | *>          Remove zone/pattern from channel B

Modes  (all UKF-filtered derivatives):
  depth   — current contact level (raw)
  speed   — |dlevel/dt|, normalised
  acc     — |d²level/dt²|, normalised
  recoil  — sudden motion changes, normalised

UKF tuning  (Unscented Kalman Filter — shared by every contact)
  ukf                                  Show current Q/R/alpha/beta/kappa
  ukf <q> <r>                          Set process / measurement noise (q,r > 0)
  ukf <q> <r> <alpha> <beta> <kappa>   Full tuning (alpha~0.5, beta=2, kappa=0)
  ukf reset                            Restore default tuning

Motion normalisation (divisors that map raw derivatives → 0..1)
  norms                                Show current speed / acc / recoil divisors
  norms <speed> <acc> <recoil>         Set all three at once (positive floats)
  norm-speed  <v>                      Set the speed divisor only
  norm-acc    <v>                      Set the acc divisor only
  norm-recoil <v>                      Set the recoil divisor only
  norms reset                          Restore defaults (speed=5, acc=30, recoil=100)

Pulse shape
  freq-a-hz <h0> <h1> <h2> <h3>   Channel A frequency in Hz (1–100) per segment
  freq-b-hz <h0> <h1> <h2> <h3>   Channel B frequency in Hz (1–100) per segment
  freq-a <v0> <v1> <v2> <v3>      Channel A frequency segments (raw 10–255)
  freq-b <v0> <v1> <v2> <v3>      Channel B frequency segments (raw 10–255)
  int-a  <v0> <v1> <v2> <v3>      Channel A intensity segments  (0–100)
  int-b  <v0> <v1> <v2> <v3>      Channel B intensity segments

Power limits
  lim-a  <min> <max>               Channel A strength range (0–200)
  lim-b  <min> <max>               Channel B strength range (0–200)
  agg-a  max|sum|avg               Channel A aggregation mode
  agg-b  max|sum|avg               Channel B aggregation mode

Dynamic modulation (per-segment mathematical functions driven by UKF)
  mod-freq-a <seg> <func> <src> [key=val ...]   Set freq modulation on channel A
  mod-freq-b <seg> <func> <src> [key=val ...]   Set freq modulation on channel B
  mod-int-a  <seg> <func> <src> [key=val ...]   Set intensity modulation on channel A
  mod-int-b  <seg> <func> <src> [key=val ...]   Set intensity modulation on channel B
  mod-show / mod-show-a / mod-show-b            Show modulation config
  mod-off-a  <seg|all> [freq|int|both]          Disable modulation on channel A
  mod-off-b  <seg|all> [freq|int|both]          Disable modulation on channel B
  Functions: sin cos tan sinh cosh tanh x2 x3 sqrt sigmoid triangle saw
    perlin simplex fractal sin+noise sin*noise ... (use mod-freq-a for full list)
  Sources: depth speed acc recoil
  Params: sens=<f> dev=<f> phase=<f> fmul=<f> off=<f> pow=<f> min=<f> max=<f>

Info / config
  zones                            List all avatar zones + which channel uses them
  status                           Current levels, strength and active zones
  mon on|off                       Toggle live power-stream printout (default: on)
  config                           Print full config
  save                             Save config to cli_config.json
  load                             Load config from cli_config.json

General
  help / h / ?                     Show this help
  clear / cls                      Clear screen
  quit / exit / q                  Stop and exit
")?;
            Ok(())
        })
    }
}
