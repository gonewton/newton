//! Natural-language command router (feature `ask`).
//!
//! v1 ranks commands via case-insensitive substring + token-overlap scoring
//! over each command's `summary`/`syntax`/`category`.  No LLM call.

use anyhow::{anyhow, Result};

pub mod error_codes {
    pub const CLI_ASK_001: &str = "CLI-ASK-001";
    pub const CLI_ASK_002: &str = "CLI-ASK-002";
}

#[derive(Debug, Clone)]
pub struct CommandSummary {
    pub name: String,
    pub summary: String,
    pub syntax: String,
    pub category: String,
}

#[derive(Debug, Clone)]
pub struct Ranked {
    pub name: String,
    pub score: f32,
}

pub trait CommandMatcher {
    fn rank(&self, query: &str, summaries: &[CommandSummary]) -> Vec<Ranked>;
}

pub struct SubstringMatcher;

impl CommandMatcher for SubstringMatcher {
    fn rank(&self, query: &str, summaries: &[CommandSummary]) -> Vec<Ranked> {
        let q = query.to_ascii_lowercase();
        let q_tokens: Vec<&str> = q
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| !t.is_empty())
            .collect();
        let mut ranked: Vec<Ranked> = summaries
            .iter()
            .map(|s| {
                let mut score = 0.0f32;
                let hay = format!(
                    "{} {} {} {}",
                    s.name.to_ascii_lowercase(),
                    s.summary.to_ascii_lowercase(),
                    s.syntax.to_ascii_lowercase(),
                    s.category.to_ascii_lowercase()
                );
                if hay.contains(&q) {
                    score += 5.0;
                }
                if s.name.to_ascii_lowercase() == q {
                    score += 10.0;
                }
                for tok in &q_tokens {
                    if s.name.to_ascii_lowercase().contains(tok) {
                        score += 2.0;
                    }
                    if hay.contains(tok) {
                        score += 1.0;
                    }
                }
                Ranked {
                    name: s.name.clone(),
                    score,
                }
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        ranked
    }
}

pub fn run(query: &str, summaries: &[CommandSummary]) -> Result<()> {
    if query.trim().is_empty() {
        return Err(anyhow!(
            "{}: ask requires a non-empty query string",
            error_codes::CLI_ASK_001
        ));
    }
    let ranked = SubstringMatcher.rank(query, summaries);
    let top: Vec<&Ranked> = ranked.iter().filter(|r| r.score > 0.0).take(3).collect();
    if top.is_empty() {
        return Err(anyhow!(
            "{}: no matching command for query '{}'",
            error_codes::CLI_ASK_002,
            query
        ));
    }
    for r in &top {
        if let Some(s) = summaries.iter().find(|s| s.name == r.name) {
            println!("{}  ({})  — {}", s.name, s.category, s.summary);
            if !s.syntax.is_empty() {
                println!("    {}", s.syntax);
            }
        }
    }
    Ok(())
}
