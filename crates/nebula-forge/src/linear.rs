//! LINEAR ISSUES: the ISSUES MODAL's four calls — list, comments, comment,
//! edit — for a project whose `issues` setting names a Linear team
//! (`linear:REL`, [`crate::issues::IssueSource::Linear`]). They go to
//! Linear's GraphQL API through `curl`, as the update check already runs
//! it, so there is no HTTP crate.
//!
//! The key is `LINEAR_API_KEY` from the environment, and it reaches `curl`
//! on stdin only: one config file (`-K -`) carries the `Authorization`
//! header and the request body, so neither the key nor a comment's text is
//! ever in argv, where `ps` shows it. Nothing here logs, and nothing writes
//! the key anywhere.

use nebula_core::env::LINEAR_API_KEY;
use serde_json::{json, Value};

use crate::issues::{Issue, IssueComment, IssueDetail, IssueText, LIST_LIMIT};

const ENDPOINT: &str = "https://api.linear.app/graphql";
/// The budget a call gets, `gh`'s: past it the `curl` is killed.
const TIMEOUT: std::time::Duration = crate::pull_request::TIMEOUT;

const LIST: &str = "query($team: String!, $first: Int!) {
  issues(first: $first, filter: {
    team: { key: { eq: $team } },
    state: { type: { nin: [\"completed\", \"canceled\"] } }
  }) {
    nodes {
      identifier url title description createdAt updatedAt
      creator { displayName }
      labels { nodes { name } }
    }
  }
}";

const DETAIL: &str = "query($id: String!) {
  issue(id: $id) {
    url
    comments { nodes { body createdAt user { displayName } } }
  }
}";

const COMMENT: &str = "mutation($id: String!, $body: String!) {
  commentCreate(input: { issueId: $id, body: $body }) { success }
}";

const EDIT: &str = "mutation($id: String!, $title: String!, $body: String!) {
  issueUpdate(id: $id, input: { title: $title, description: $body }) { success }
}";

/// The team's open issues — every state but completed and canceled —
/// newest first. `None` is "couldn't ask".
pub async fn list(team: &str) -> Option<Vec<Issue>> {
    let first = LIST_LIMIT as u64;
    let data = call(LIST, json!({ "team": team, "first": first }))
        .await
        .ok()?;
    parse_list(&data)
}

/// One issue's comments, oldest first.
pub async fn detail(key: &str) -> Option<IssueDetail> {
    let data = call(DETAIL, json!({ "id": key })).await.ok()?;
    parse_detail(&data)
}

/// Post `body` on the issue as the key's owner. True when Linear took it.
pub async fn comment(key: &str, body: &str) -> bool {
    call(COMMENT, json!({ "id": key, "body": body }))
        .await
        .is_ok_and(|data| succeeded(&data, "commentCreate"))
}

/// Give the issue a new title and description; `Err` is Linear's reason.
pub async fn edit(key: &str, text: &IssueText) -> Result<(), String> {
    let data = call(
        EDIT,
        json!({ "id": key, "title": text.title, "body": text.body }),
    )
    .await?;
    if succeeded(&data, "issueUpdate") {
        Ok(())
    } else {
        Err("Linear refused the edit".into())
    }
}

fn succeeded(data: &Value, mutation: &str) -> bool {
    data.pointer(&format!("/{mutation}/success"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// One GraphQL request: its `data`, or why there is none — the first error
/// Linear gave, or what went wrong before it could answer.
async fn call(query: &str, variables: Value) -> Result<Value, String> {
    let key = std::env::var(LINEAR_API_KEY)
        .ok()
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| format!("{LINEAR_API_KEY} is not set"))?;
    let body = json!({ "query": query, "variables": variables }).to_string();
    let out = run_curl(&curl_config(key.trim(), &body)).await?;
    answer(&out)
}

/// The `curl` config fed on stdin: the key's header and the request body,
/// each a double-quoted value with `\` and `"` escaped, curl's quoting.
/// A body is JSON, so it holds no raw newline and never opens with `@`.
fn curl_config(key: &str, body: &str) -> String {
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "header = \"Authorization: {}\"\ndata-binary = \"{}\"\n",
        quote(key),
        quote(body)
    )
}

/// What Linear sent back: `data`, unless it named an error.
fn answer(out: &str) -> Result<Value, String> {
    let v: Value =
        serde_json::from_str(out).map_err(|_| "Linear's answer wasn't JSON".to_string())?;
    if let Some(error) = v.pointer("/errors/0/message").and_then(Value::as_str) {
        return Err(format!("Linear: {error}"));
    }
    match v.get("data") {
        Some(data) if !data.is_null() => Ok(data.clone()),
        _ => Err("Linear sent no data".into()),
    }
}

async fn run_curl(config: &str) -> Result<String, String> {
    use tokio::io::AsyncWriteExt;
    let max_time = TIMEOUT.as_secs().to_string();
    let mut child = tokio::process::Command::new("curl")
        .args(["-sS", "--max-time", &max_time])
        .args(["-H", "Content-Type: application/json", "-K", "-", ENDPOINT])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("couldn't run curl: {e}"))?;
    let Some(mut stdin) = child.stdin.take() else {
        return Err("couldn't feed curl".into());
    };
    let config = config.to_string();
    let feed = async move {
        stdin.write_all(config.as_bytes()).await?;
        stdin.shutdown().await
    };
    // Feed and wait together, as `gh issue comment` is fed: a `curl` that
    // exits before reading must not leave the write blocked.
    let run = async {
        let (_, out) = tokio::join!(feed, child.wait_with_output());
        match out {
            Ok(out) if out.status.success() => Ok(String::from_utf8_lossy(&out.stdout).into()),
            Ok(out) => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
            Err(e) => Err(format!("curl failed: {e}")),
        }
    };
    tokio::time::timeout(TIMEOUT, run)
        .await
        .unwrap_or_else(|_| Err("Linear timed out".into()))
}

fn text_at(v: &Value, pointer: &str) -> String {
    v.pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// `data.issues.nodes` as rows, newest first. A row whose URL the DAEMON
/// would refuse ([`nebula_core::linear_issue_key`]) drops out, as a GitHub
/// row with no openable URL does.
fn parse_list(data: &Value) -> Option<Vec<Issue>> {
    let nodes = data.pointer("/issues/nodes")?.as_array()?;
    let mut list: Vec<Issue> = nodes
        .iter()
        .filter_map(|v| {
            let url = text_at(v, "/url");
            let key = nebula_core::linear_issue_key(&url)?.to_string();
            Some(Issue {
                key,
                title: text_at(v, "/title"),
                author: text_at(v, "/creator/displayName"),
                created_at: text_at(v, "/createdAt"),
                updated_at: text_at(v, "/updatedAt"),
                labels: v
                    .pointer("/labels/nodes")
                    .and_then(Value::as_array)
                    .map(|labels| {
                        labels
                            .iter()
                            .map(|l| text_at(l, "/name"))
                            .filter(|n| !n.is_empty())
                            .collect()
                    })
                    .unwrap_or_default(),
                body: text_at(v, "/description"),
                url,
            })
        })
        .collect();
    // ISO 8601 UTC stamps sort lexicographically into chronological order.
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Some(list)
}

fn parse_detail(data: &Value) -> Option<IssueDetail> {
    let issue = data.get("issue")?;
    let url = text_at(issue, "/url");
    nebula_core::linear_issue_key(&url)?;
    let mut comments: Vec<IssueComment> = issue
        .pointer("/comments/nodes")
        .and_then(Value::as_array)
        .map(|nodes| {
            nodes
                .iter()
                .map(|c| IssueComment {
                    author: text_at(c, "/user/displayName"),
                    at: text_at(c, "/createdAt"),
                    body: text_at(c, "/body"),
                })
                .collect()
        })
        .unwrap_or_default();
    comments.sort_by(|a, b| a.at.cmp(&b.at));
    Some(IssueDetail { url, comments })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_reads_linear_nodes_newest_first() {
        let data = json!({ "issues": { "nodes": [
            {
                "identifier": "REL-1", "url": "https://linear.app/acme/issue/REL-1/old",
                "title": "Old", "description": null,
                "createdAt": "2026-01-01T00:00:00.000Z", "updatedAt": "2026-01-02T00:00:00.000Z",
                "creator": { "displayName": "isac" }, "labels": { "nodes": [{ "name": "Bug" }] }
            },
            {
                "identifier": "REL-2", "url": "https://linear.app/acme/issue/REL-2/new",
                "title": "New", "description": "Body",
                "createdAt": "2026-02-01T00:00:00.000Z", "updatedAt": "2026-02-01T00:00:00.000Z",
                "creator": null, "labels": { "nodes": [] }
            },
            { "identifier": "X-1", "url": "https://evil.dev/x", "title": "Dropped" }
        ] } });
        let list = parse_list(&data).unwrap();
        let keys: Vec<&str> = list.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, ["REL-2", "REL-1"]);
        assert_eq!(list[1].author, "isac");
        assert_eq!(list[1].labels, ["Bug"]);
        assert_eq!(list[1].body, "", "a null description reads as empty");
        assert_eq!(list[0].label(), "REL-2 New");
        assert!(parse_list(&json!({})).is_none());
    }

    #[test]
    fn the_detail_reads_comments_oldest_first() {
        let data = json!({ "issue": {
            "url": "https://linear.app/acme/issue/REL-2/new",
            "comments": { "nodes": [
                { "body": "second", "createdAt": "2026-02-02T00:00:00.000Z", "user": null },
                { "body": "first", "createdAt": "2026-02-01T00:00:00.000Z", "user": { "displayName": "isac" } }
            ] }
        } });
        let detail = parse_detail(&data).unwrap();
        assert_eq!(detail.comments[0].body, "first");
        assert_eq!(detail.comments[0].author, "isac");
        assert_eq!(detail.comments[1].author, "");
    }

    #[test]
    fn an_error_answer_is_linears_first_message() {
        assert_eq!(
            answer(r#"{"errors":[{"message":"Authentication required"}]}"#),
            Err("Linear: Authentication required".into())
        );
        assert_eq!(answer("<html>"), Err("Linear's answer wasn't JSON".into()));
        assert_eq!(answer(r#"{"data":{"a":1}}"#), Ok(json!({ "a": 1 })));
    }

    /// The key and the body ride stdin as a curl config, quoted so a `"`
    /// or `\` in either can't end the value early.
    #[test]
    fn the_curl_config_quotes_the_key_and_body() {
        let body = json!({ "query": "q", "variables": { "body": "say \"hi\"\n\\o/" } }).to_string();
        let config = curl_config("lin_api_x", &body);
        let mut lines = config.lines();
        assert_eq!(lines.next(), Some("header = \"Authorization: lin_api_x\""));
        let data = lines.next().unwrap();
        assert_eq!(
            data,
            r#"data-binary = "{\"query\":\"q\",\"variables\":{\"body\":\"say \\\"hi\\\"\\n\\\\o/\"}}""#
        );
        assert_eq!(lines.next(), None);
    }
}
