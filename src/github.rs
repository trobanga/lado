use anyhow::{anyhow, Context, Result};
use std::process::Command;

/// Represents PR branch information
#[derive(Debug)]
#[allow(dead_code)]
pub struct PrInfo {
    pub base_ref: String,
    pub head_ref: String,
    pub title: String,
}

/// Fetch PR information using the gh CLI
pub fn get_pr_info(pr_number: u32) -> Result<PrInfo> {
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--json",
            "baseRefName,headRefName,title",
        ])
        .output()
        .context("Failed to execute gh CLI. Is it installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("gh pr view failed: {}", stderr));
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse gh output")?;

    let base_ref = json["baseRefName"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing baseRefName"))?
        .to_string();

    let head_ref = json["headRefName"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing headRefName"))?
        .to_string();

    let title = json["title"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing title"))?
        .to_string();

    Ok(PrInfo {
        base_ref,
        head_ref,
        title,
    })
}

/// Get the base and head refs for a PR
pub fn get_pr_refs(pr_number: u32) -> Result<(String, String)> {
    let info = get_pr_info(pr_number)?;
    Ok((info.base_ref, info.head_ref))
}
