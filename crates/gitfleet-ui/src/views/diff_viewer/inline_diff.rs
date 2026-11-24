//! Intra-line token diff computation using prefix/suffix trimming
//! and identifier-weighted longest common subsequence (LCS).

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InlineSpan {
    pub(crate) text: String,
    pub(crate) highlighted: bool,
}

fn tokenize_code(s: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let ch = s[i..].chars().next().unwrap();
        if ch.is_alphanumeric() || ch == '_' {
            i += ch.len_utf8();
            while i < bytes.len() {
                let c = s[i..].chars().next().unwrap();
                if c.is_alphanumeric() || c == '_' {
                    i += c.len_utf8();
                } else {
                    break;
                }
            }
        } else if ch.is_whitespace() {
            i += ch.len_utf8();
            while i < bytes.len() {
                let c = s[i..].chars().next().unwrap();
                if c.is_whitespace() {
                    i += c.len_utf8();
                } else {
                    break;
                }
            }
        } else {
            i += ch.len_utf8();
        }
        tokens.push(&s[start..i]);
    }
    tokens
}

fn token_weight(tok: &str) -> usize {
    let first = tok.chars().next().unwrap_or(' ');
    if first.is_alphanumeric() || first == '_' {
        tok.chars().count().max(1) * 3
    } else if first.is_whitespace() {
        1
    } else {
        2
    }
}

fn coalesce_spans(tokens: &[&str], changed: &[bool]) -> Vec<InlineSpan> {
    let mut out: Vec<InlineSpan> = Vec::new();
    for (&tok, &is_chg) in tokens.iter().zip(changed.iter()) {
        if let Some(last) = out.last_mut()
            && last.highlighted == is_chg
        {
            last.text.push_str(tok);
        } else {
            out.push(InlineSpan {
                text: tok.to_string(),
                highlighted: is_chg,
            });
        }
    }
    out
}

pub(crate) fn full_changed_spans(line: &str) -> Vec<InlineSpan> {
    if line.is_empty() {
        return Vec::new();
    }
    let trimmed_start = line.len() - line.trim_start().len();
    if trimmed_start > 0 && trimmed_start < line.len() {
        vec![
            InlineSpan {
                text: line[..trimmed_start].to_string(),
                highlighted: false,
            },
            InlineSpan {
                text: line[trimmed_start..].to_string(),
                highlighted: true,
            },
        ]
    } else {
        vec![InlineSpan {
            text: line.to_string(),
            highlighted: true,
        }]
    }
}

/// Computes intra-line token diff highlights for a modified `(old, new)` line pair
/// using prefix/suffix trimming + identifier-weighted LCS.
pub(crate) fn compute_inline_diff(old: &str, new: &str) -> (Vec<InlineSpan>, Vec<InlineSpan>) {
    if old == new {
        return (Vec::new(), Vec::new());
    }
    let a = tokenize_code(old);
    let b = tokenize_code(new);
    if a.is_empty() || b.is_empty() {
        return (full_changed_spans(old), full_changed_spans(new));
    }

    let mut prefix = 0;
    while prefix < a.len() && prefix < b.len() && a[prefix] == b[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < a.len() - prefix
        && suffix < b.len() - prefix
        && a[a.len() - 1 - suffix] == b[b.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let a_mid = &a[prefix..a.len() - suffix];
    let b_mid = &b[prefix..b.len() - suffix];

    let mut a_chg = vec![false; a.len()];
    let mut b_chg = vec![false; b.len()];

    if a_mid.is_empty() || b_mid.is_empty() || a_mid.len() > 160 || b_mid.len() > 160 {
        for flag in &mut a_chg[prefix..a.len() - suffix] {
            *flag = true;
        }
        for flag in &mut b_chg[prefix..b.len() - suffix] {
            *flag = true;
        }
    } else {
        let m = a_mid.len();
        let n = b_mid.len();
        let cols = n + 1;
        let mut dp = vec![0usize; (m + 1) * cols];
        for i in (0..m).rev() {
            for j in (0..n).rev() {
                dp[i * cols + j] = if a_mid[i] == b_mid[j] {
                    token_weight(a_mid[i]) + dp[(i + 1) * cols + (j + 1)]
                } else {
                    dp[(i + 1) * cols + j].max(dp[i * cols + (j + 1)])
                };
            }
        }

        // Backtrack to find unmatched tokens in `a_mid` and `b_mid`
        let mut i = 0;
        let mut j = 0;
        let mut matched_weight = 0usize;
        while i < m && j < n {
            if a_mid[i] == b_mid[j]
                && dp[i * cols + j] == token_weight(a_mid[i]) + dp[(i + 1) * cols + (j + 1)]
            {
                matched_weight += token_weight(a_mid[i]);
                i += 1;
                j += 1;
            } else if dp[(i + 1) * cols + j] >= dp[i * cols + (j + 1)] {
                a_chg[prefix + i] = true;
                i += 1;
            } else {
                b_chg[prefix + j] = true;
                j += 1;
            }
        }
        while i < m {
            a_chg[prefix + i] = true;
            i += 1;
        }
        while j < n {
            b_chg[prefix + j] = true;
            j += 1;
        }

        // If the entire line shares almost no meaningful tokens, highlight full changed middle cleanly
        let total_weight: usize = a.iter().map(|t| token_weight(t)).sum::<usize>()
            + b.iter().map(|t| token_weight(t)).sum::<usize>();
        let prefix_suffix_weight: usize = a[..prefix]
            .iter()
            .chain(&a[a.len() - suffix..])
            .map(|t| token_weight(t))
            .sum::<usize>()
            * 2;
        if total_weight > 0 && (prefix_suffix_weight + matched_weight * 2) * 4 < total_weight {
            return (full_changed_spans(old), full_changed_spans(new));
        }

        // Smooth single punctuation/space tokens trapped directly between two changed spans
        let smooth = |tokens: &[&str], flags: &mut [bool]| {
            for k in 1..flags.len().saturating_sub(1) {
                if !flags[k]
                    && flags[k - 1]
                    && flags[k + 1]
                    && tokens[k]
                        .chars()
                        .all(|c| c.is_whitespace() || c == ',' || c == '*')
                {
                    flags[k] = true;
                }
            }
        };
        smooth(&a, &mut a_chg);
        smooth(&b, &mut b_chg);
    }

    (coalesce_spans(&a, &a_chg), coalesce_spans(&b, &b_chg))
}
