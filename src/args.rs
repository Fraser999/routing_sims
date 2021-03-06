// Copyright 2016 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under (1) the MaidSafe.net Commercial License,
// version 1.0 or later, or (2) The General Public License (GPL), version 3, depending on which
// licence you accepted on initial access to the Software (the "Licences").
//
// By contributing code to the SAFE Network Software, or to this project generally, you agree to be
// bound by the terms of the MaidSafe Contributor Agreement, version 1.1.  This, along with the
// Licenses can be found in the root directory of this project at LICENSE, COPYING and CONTRIBUTOR.
//
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.
//
// Please review the Licences for the specific language governing permissions and limitations
// relating to use of the SAFE Network Software.

//! Argument processing

use docopt::Docopt;
use super::{ToolArgs, NN, RR};
use super::tools::{Tool, DirectCalcTool, SimStructureTool, FullSimTool, SimResult};
use super::quorum::*;

use std::str::FromStr;
use std::fmt::Debug;
use std::ops::AddAssign;
use std::cmp::Ordering;


#[cfg_attr(rustfmt, rustfmt_skip)]
const USAGE: &'static str = "
Probability computation tool.

Usage:
    routing-sims [-h | --help]
    routing-sims calc \
     [-n RANGE] [-r RANGE] [-k RANGE] [-q RANGE] [-s VAL] [-p VAL]
    routing-sims structure [-n \
     RANGE] [-r RANGE] [-k RANGE] [-q RANGE] [-s VAL] [-p VAL]
    routing-sims full [-n RANGE] \
     [-r RANGE] [-k RANGE] [-q RANGE] [-s VAL] [-p VAL] [-Q QTYPE] [-T TTYPE]

Tools:
    calc        Direct calculation: all groups have min size, no ageing or targetting
    structure   Simulate group structure, but no ageing or targetting
    full        Full simulation (see -Q and -T parameters)

Options:
    -h --help   Show this message
    -n RANGE    Number of nodes, total, e.g. 1000-5000:1000.
    -r RANGE    Either number of compromised nodes (e.g. 50) or percentage (default is 10%).
    -k RANGE    Minimum group size, e.g. 10-20.
    -q RANGE    Quorum size as a proportion with step size, e.g. 0.5-0.7:0.1.
    -s VAL      Maximum number of steps, each the length of one proof-of-work.
    -p VAL      Number of times to repeat a true/false simulation to calculate
                an attack success probability.
    -Q QTYPE    Quorum algorithm: simple, age or all
    -T TTYPE    Attack targetting strategy: none, simple or all
";

#[allow(non_snake_case)]
#[derive(RustcDecodable)]
struct Args {
    cmd_calc: bool,
    cmd_structure: bool,
    cmd_full: bool,
    flag_n: Option<String>,
    flag_r: Option<String>,
    flag_k: Option<String>,
    flag_q: Option<String>,
    flag_s: Option<NN>,
    flag_p: Option<NN>,
    flag_Q: Option<String>,
    flag_T: Option<String>,
}

pub trait DefaultStep<T> {
    // Return a default step.
    //
    // The value `x` is passed so that RelOrAbs can see whether it's being
    // used in relative or absolute form.
    fn default_step(x: T) -> T;
}

impl DefaultStep<NN> for NN {
    fn default_step(_: NN) -> NN {
        1
    }
}

impl DefaultStep<RR> for RR {
    fn default_step(_: RR) -> RR {
        1.0
    }
}

pub enum SamplePoints<T> {
    Range(T, T, Option<T>), // start, stop, optional step
    List(Vec<T>),
    Number(T),
}

impl<T: Copy + Debug + AddAssign + PartialOrd<T> + DefaultStep<T>> SamplePoints<T> {
    fn iter(&self) -> SamplePointsIterator<T> {
        SamplePointsIterator {
            iterable: self,
            i: 0,
            prev: None,
        }
    }
}

impl<T: FromStr> FromStr for SamplePoints<T>
    where <T as FromStr>::Err: Debug
{
    type Err = ();  // we just panic!
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains('-') {
            // We have a range; check for a step:
            let (first, step) = if s.contains(':') {
                let mut parts = s.split(':');
                let first = parts.next().expect("split half");
                let second = parts.next().expect("split half");
                if parts.next() != None {
                    panic!("expected 'start-stop:step', found {}", s);
                }
                (first, Some(second.parse().expect("parse")))
            } else {
                (s, None)
            };
            let mut parts = first.split('-');
            let start = parts.next().expect("split half").parse().expect("parse");
            let stop = match parts.next() {
                    Some(part) => part,
                    None => panic!("expected 'start-stop:step', found {}", s),
                }
                .parse()
                .expect("parse");
            if parts.next() != None {
                panic!("expected 'start-stop:step', found {}", s);
            }
            Ok(SamplePoints::Range(start, stop, step))
        } else if s.contains(',') {
            // We have a list
            let parts = s.split(',');
            Ok(SamplePoints::List(parts.map(|p| p.parse().expect("parse")).collect()))
        } else {
            // Presumably we have a single number
            Ok(SamplePoints::Number(s.parse().expect("parse")))
        }
    }
}

struct SamplePointsIterator<'a, T: Copy + Debug + AddAssign + PartialOrd<T> + DefaultStep<T> + 'a> {
    iterable: &'a SamplePoints<T>,
    i: usize,
    prev: Option<T>,
}

impl<'a, T: Copy + Debug + AddAssign + PartialOrd<T> + DefaultStep<T> + 'a> Iterator
        for SamplePointsIterator<'a, T>
{
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        let i = self.i;
        match self.iterable {
            &SamplePoints::Range(start, stop, step) => {
                match self.prev {
                    None => {
                        self.prev = Some(start);
                        self.prev
                    },
                    Some(mut x) => {
                        let step = step.unwrap_or(T::default_step(start));
                        x += step;
                        self.prev = Some(x);
                        if x > stop {
                            None
                        } else {
                            Some(x)
                        }
                    },
                }
            },
            &SamplePoints::List(ref v) => {
                if i >= v.len() {
                    None
                } else {
                    self.i = i + 1;
                    Some(v[i])
                }
            },
            &SamplePoints::Number(n) => {
                if i > 0 {
                    None
                } else {
                    self.i = 1;
                    Some(n)
                }
            },
        }
    }
}

pub struct ArgProc {
    args: Args,
}

impl ArgProc {
    pub fn read_args() -> ArgProc {
        let args: Args = Docopt::new(USAGE)
            .and_then(|dopt| dopt.decode())
            .unwrap_or_else(|e| e.exit());

        ArgProc { args: args }
    }

    // TODO: is Vec suitable for this use?
    pub fn make_sim_params(&self) -> Vec<SimParams> {
        let mut v = Vec::new();

        let nodes_range: SamplePoints<NN> = self.args
            .flag_n
            .as_ref()
            .map_or(SamplePoints::Number(1000), |s| s.parse().expect("parse"));
        let mut nodes_iter = nodes_range.iter();

        let mal_nodes_range: SamplePoints<RelOrAbs> =
            self.args.flag_r.as_ref().map_or(SamplePoints::Number(RelOrAbs::Rel(0.1)),
                                             |s| s.parse().expect("parse"));
        let mut mal_nodes_iter = mal_nodes_range.iter();

        let group_size_range: SamplePoints<NN> = self.args
            .flag_k
            .as_ref()
            .map_or(SamplePoints::Number(10), |s| s.parse().expect("parse"));
        let mut group_size_iter = group_size_range.iter();

        let quorum_range = self.args
            .flag_q
            .as_ref()
            .map_or(SamplePoints::Number(0.5), |s| s.parse().expect("parse"));
        let mut quorum_iter = quorum_range.iter();

        let q_use_age = match self.args.flag_Q.as_ref().map(|s| s.as_str()) {
            None => vec![false],
            Some("simple") => vec![false],
            Some("age") => vec![true],
            Some("all") => vec![false, true],
            Some(x) => panic!("unexpected: -Q {}", x),
        };
        let mut q_use_age_iter = q_use_age.iter();

        let at_type = match self.args.flag_T.as_ref().map(|s| s.as_str()) {
            None => vec![AttackType::Untargetted],
            Some("none") => vec![AttackType::Untargetted],
            Some("simple") => vec![AttackType::SimpleTargetted],
            Some("all") => vec![AttackType::Untargetted, AttackType::SimpleTargetted],
            Some(x) => panic!("unexpected: -T {}", x),
        };
        let mut at_type_iter = at_type.iter();

        // Create initial parameter set
        let tool = if self.args.cmd_calc {
            SimType::DirectCalc
        } else if self.args.cmd_structure {
            SimType::Structure
        } else if self.args.cmd_full {
            SimType::FullSim
        } else {
            unreachable!()
        };
        v.push(SimParams {
            sim_type: tool,
            age_quorum: *q_use_age_iter.next().expect("first iter item"),
            targetting: *at_type_iter.next().expect("first iter item"),
            num_nodes: nodes_iter.next().expect("first iter item"),
            num_malicious: mal_nodes_iter.next().expect("first iter item"),
            min_group_size: group_size_iter.next().expect("first iter item"),
            quorum_prop: quorum_iter.next().expect("first iter item"),
            max_steps: self.args.flag_s.unwrap_or(1000),
            repetitions: self.args.flag_p.unwrap_or(100),
        });

        // Replicate for all network sizes (num nodes)
        let range = 0..v.len();
        for n in nodes_iter {
            for i in range.clone() {
                let mut s = v[i].clone();
                s.num_nodes = n;
                v.push(s);
            }
        }

        // Replicate for all numbers of malicious nodes
        let range = 0..v.len();
        for r in mal_nodes_iter {
            for i in range.clone() {
                let mut s = v[i].clone();
                // NOTE: it's important that we replicate over num_nodes first!
                s.num_malicious = r;
                v.push(s);
            }
        }

        // Replicate for all group sizes
        let range = 0..v.len();
        for g in group_size_iter {
            for i in range.clone() {
                let mut s = v[i].clone();
                s.min_group_size = g;
                v.push(s);
            }
        }

        // Replicate for all quorum sizes
        let range = 0..v.len();
        for q in quorum_iter {
            for i in range.clone() {
                let mut s = v[i].clone();
                s.quorum_prop = q;
                v.push(s);
            }
        }

        // Replicate for all quorum types
        let range = 0..v.len();
        for q in q_use_age_iter {
            for i in range.clone() {
                let mut s = v[i].clone();
                s.age_quorum = *q;
                v.push(s);
            }
        }

        // Replicate for all attack strategies
        let range = 0..v.len();
        for at in at_type_iter {
            for i in range.clone() {
                let mut s = v[i].clone();
                s.targetting = *at;
                v.push(s);
            }
        }

        v
    }
}

#[derive(Clone, Copy)]
pub enum SimType {
    DirectCalc,
    Structure,
    FullSim,
}

impl SimType {
    pub fn name(self) -> &'static str {
        match self {
            SimType::DirectCalc => "dir_calc",
            SimType::Structure => "structure",
            SimType::FullSim => "full_sim",
        }
    }
}

#[derive(Clone, Copy)]

pub enum AttackType {
    Untargetted,
    SimpleTargetted,
}

impl AttackType {
    pub fn name(&self) -> &'static str {
        match self {
            &AttackType::Untargetted => "untarg.",
            &AttackType::SimpleTargetted => "simp_targ",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RelOrAbs {
    Rel(RR),
    Abs(NN),
}

impl RelOrAbs {
    pub fn from_base(self, base: NN) -> NN {
        match self {
            RelOrAbs::Rel(r) => ((base as RR) * r) as NN,
            RelOrAbs::Abs(n) => n,
        }
    }
}

impl FromStr for RelOrAbs {
    type Err = ();  // we just panic!
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.ends_with('%') {
            let mut s = s.to_string();
            let _ = s.pop();
            let perc = s.parse::<RR>().expect("parse");
            Ok(RelOrAbs::Rel(perc * 0.01))
        } else {
            Ok(RelOrAbs::Abs(s.parse().expect("parse")))
        }
    }
}

impl AddAssign for RelOrAbs {
    fn add_assign(&mut self, rhs: RelOrAbs) {
        match (self, rhs) {
            (&mut RelOrAbs::Rel(ref mut x), RelOrAbs::Rel(y)) => *x += y,
            (&mut RelOrAbs::Abs(ref mut x), RelOrAbs::Abs(y)) => *x += y,
            _ => panic!("wrong rel/abs type!"),
        }
    }
}

impl PartialOrd<RelOrAbs> for RelOrAbs {
    fn partial_cmp(&self, rhs: &RelOrAbs) -> Option<Ordering> {
        match (self, rhs) {
            (&RelOrAbs::Rel(x), &RelOrAbs::Rel(ref y)) => x.partial_cmp(y),
            (&RelOrAbs::Abs(x), &RelOrAbs::Abs(ref y)) => x.partial_cmp(y),
            _ => panic!("wrong rel/abs type!"),
        }
    }
}

impl DefaultStep<RelOrAbs> for RelOrAbs {
    fn default_step(x: RelOrAbs) -> RelOrAbs {
        match x {
            RelOrAbs::Rel(_) => RelOrAbs::Rel(0.1),
            RelOrAbs::Abs(_) => RelOrAbs::Abs(1),
        }
    }
}

pub const PARAM_TITLES: [&'static str; 9] = ["Type",
                                             "AgeQuorum",
                                             "Targetting",
                                             "Nodes",
                                             "Malicious",
                                             "MinGroup",
                                             "QuorumProp",
                                             "P(disruption)",
                                             "P(compromise)"];
#[derive(Clone)]
pub struct SimParams {
    pub sim_type: SimType,
    pub age_quorum: bool,
    pub targetting: AttackType,
    pub num_nodes: NN,
    pub num_malicious: RelOrAbs,
    pub min_group_size: NN,
    pub quorum_prop: RR,
    pub max_steps: NN,
    pub repetitions: NN,
}

impl SimParams {
    pub fn result(&self) -> SimResult {
        let args = ToolArgs {
            num_nodes: self.num_nodes,
            num_malicious: self.num_malicious.from_base(self.num_nodes),
            min_group_size: self.min_group_size,
            quorum_prop: self.quorum_prop,
            any_group: true, // only support this mode now
            max_steps: self.max_steps,
            repetitions: self.repetitions,
        };
        args.check_invariant();

        let tool: Box<Tool> = match self.sim_type {
            SimType::DirectCalc => Box::new(DirectCalcTool::new(args)),
            SimType::Structure => Box::new(SimStructureTool::new(args)),
            SimType::FullSim => {
                // note: FullSimTool is templated on quorum and attack strategy parameters, so
                // we need to create the whole thing at once (not create parameters first)
                match (self.age_quorum, self.targetting) {
                    (false, AttackType::Untargetted) => {
                        Box::new(FullSimTool::new(args, SimpleQuorum::new(), UntargettedAttack {}))
                    }
                    (true, AttackType::Untargetted) => {
                        Box::new(FullSimTool::new(args, AgeQuorum::new(), UntargettedAttack {}))
                    }
                    (false, AttackType::SimpleTargetted) => {
                        Box::new(FullSimTool::new(args,
                                                  SimpleQuorum::new(),
                                                  SimpleTargettedAttack::new()))
                    }
                    (true, AttackType::SimpleTargetted) => {
                        Box::new(FullSimTool::new(args,
                                                  AgeQuorum::new(),
                                                  SimpleTargettedAttack::new()))
                    }
                }
            }
        };

        tool.calc_p_compromise()
    }
}
