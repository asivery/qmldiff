use anyhow::{Error, Result};
use regex::{Captures, Regex};

use crate::{hash::hash, hashtab::HashTab};

#[derive(Debug)]
enum MatchConditionEqualityCheck {
    Hash(u64),
    String(String),
    Any,
}

impl MatchConditionEqualityCheck {
    fn matches(&self, value: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Hash(h) => hash(value) == *h,
            Self::String(v) => value == v,
        }
    }
}

#[derive(Debug)]
struct MatchCondition {
    regex: Regex,
    equality_checks: Vec<MatchConditionEqualityCheck>,
}

impl MatchCondition {
    fn compile<'a, F>(regex: &str, lines: &mut F) -> Result<Self>
    where
        F: Iterator<Item = &'a str>,
    {
        let mut res = Self {
            regex: Regex::new(regex)?,
            equality_checks: Vec::new(),
        };
        for _ in 0..res.regex.captures_len() {
            let line = lines.next();
            match line {
                Some(l) if !l.is_empty() => {
                    // Guaranteed to be at least 1 char.
                    let opcode_char = l.chars().nth(0).unwrap();
                    let rest = l[1..].to_string();
                    let result = match opcode_char {
                        '-' => MatchConditionEqualityCheck::Any,
                        'H' => MatchConditionEqualityCheck::Hash(rest.parse::<u64>()?),
                        'E' => MatchConditionEqualityCheck::String(rest),
                        e => {
                            return Err(Error::msg(format!(
                                "Cannot parse opcode char for match condition: {}",
                                e
                            )))
                        }
                    };
                    res.equality_checks.push(result);
                }
                _ => {
                    return Err(Error::msg(
                        "Invalid line encountered while parsing match condition!",
                    ));
                }
            }
        }
        Ok(res)
    }
}

#[derive(Debug)]
enum RuleCondition {
    EmitAlways,
    Match(MatchCondition),
}

#[derive(Debug)]
struct Rule {
    condition: RuleCondition,
    values: Vec<String>,
}

#[derive(Debug)]
pub struct HashRules {
    rules: Vec<Rule>,
}

impl HashRules {
    pub fn compile(contents: &str) -> Result<Self> {
        let mut lines = contents.lines();
        let mut rules = Vec::default();
        while let Some(instr_line) = lines.next() {
            if instr_line.is_empty() {
                continue;
            }
            let match_opcode = instr_line.chars().nth(0).unwrap();
            let rest = &instr_line[1..];
            let condition = match match_opcode {
                'M' => RuleCondition::Match(MatchCondition::compile(rest, &mut lines)?),
                'A' => RuleCondition::EmitAlways,
                e => {
                    return Err(Error::msg(format!("Unknown condition {}", e)));
                }
            };
            let mut values = vec![];
            for out_line in lines.by_ref() {
                if out_line == "#" {
                    break;
                } else {
                    values.push(out_line.to_string());
                }
            }
            rules.push(Rule { condition, values });
        }

        Ok(HashRules { rules })
    }

    pub fn process(&self, tab: &mut HashTab) {
        // Iterate over own rules
        macro_rules! include {
            ($val: expr, $tab: expr) => {
                let value_final = Regex::new("\\[\\[([\\d]*)\\]\\]").unwrap().replace_all(
                    &$val,
                    |h: &Captures| {
                        let hashed = h[1].parse::<u64>();
                        if let Ok(hashed) = hashed {
                            if let Some(original) = tab.get(&hashed) {
                                return original.to_string();
                            } else {
                                eprintln!("No hash {} present in hashtab!", hashed);
                            }
                        } else {
                            eprintln!("Not a valid hash {}!", h[1].to_string());
                        }

                        "INVALID!".to_string()
                    },
                );
                let h = hash(&value_final);
                $tab.insert(h, value_final.to_string());
                eprintln!(
                    "[qmldiff] [Hashtab Rule Processor]: Hashed derived '{}'",
                    &value_final
                );
            };
        }
        for rule in &self.rules {
            match &rule.condition {
                RuleCondition::EmitAlways => {
                    // Just emit the output as a hash.
                    for v in &rule.values {
                        include!(v, tab);
                    }
                }
                RuleCondition::Match(cond) => {
                    // Iterate over all entries in hashtable. Find matches
                    let mut tab_temp = HashTab::new();
                    'hashiter: for (_, string) in tab.iter() {
                        if let Some(r#match) = cond.regex.captures(string) {
                            for (i, matcher) in cond.equality_checks.iter().enumerate() {
                                if !matcher.matches(r#match.get(i).unwrap().as_str()) {
                                    continue 'hashiter;
                                }
                            }
                            // Value matches
                            // Emit.
                            for value_to_emit in &rule.values {
                                let value_final = Regex::new("\\$([\\d]*)").unwrap().replace_all(
                                    value_to_emit,
                                    |h: &Captures| {
                                        let capture_index = h[1].parse::<usize>();
                                        if let Ok(capture_index) = capture_index {
                                            if let Some(original) = r#match.get(capture_index) {
                                                return original.as_str();
                                            } else {
                                                eprintln!(
                                                    "No capture {} present in parent!",
                                                    capture_index
                                                );
                                            }
                                        } else {
                                            eprintln!("Not a valid hash {}!", &h[1]);
                                        }

                                        "INVALID!"
                                    },
                                );
                                include!(value_final, tab_temp);
                            }
                        }
                    }
                    tab.extend(tab_temp);
                }
            }
        }
    }
}
