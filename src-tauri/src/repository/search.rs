use sqlx::{Row, SqlitePool};

use crate::domain::SearchHit;

/// Translate free-form user input into a safe FTS5 MATCH expression.
///
/// FTS5's query syntax treats characters like `.`, `-`, `:`, `"`, `(`, and bare
/// words `AND`/`OR`/`NOT` as operators — any of these in raw user input causes a
/// syntax error and the whole search fails. We split the input on every
/// non-alphanumeric boundary — exactly how the `unicode61` tokenizer split the
/// indexed text — wrap each token in a quoted phrase, and append `*` so partial
/// typing ("ben" finds "Benchy"). Splitting (rather than concatenating) matters:
/// `benchy.stl` was indexed as the tokens `benchy` and `stl`, so it must become
/// `"benchy"* "stl"*`, not `"benchystl"*` (which matches nothing). Multiple
/// terms join with whitespace, which FTS5 treats as implicit AND.
fn build_match_expr(input: &str) -> String {
    input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{}\"*", term))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Run an FTS5 query, optionally restricted to entity kinds.
pub async fn search(
    pool: &SqlitePool,
    query: &str,
    kinds: Option<&[String]>,
) -> sqlx::Result<Vec<SearchHit>> {
    let match_expr = build_match_expr(query);
    if match_expr.is_empty() {
        return Ok(Vec::new());
    }

    let sql = match kinds {
        Some(ks) if !ks.is_empty() => {
            let placeholders = ks.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            format!(
                "SELECT entity_kind, entity_id, name,
                        snippet(search_index, 3, '<b>', '</b>', '…', 16) AS body
                 FROM search_index
                 WHERE search_index MATCH ? AND entity_kind IN ({})
                 ORDER BY rank LIMIT 100",
                placeholders
            )
        }
        _ => "SELECT entity_kind, entity_id, name,
                     snippet(search_index, 3, '<b>', '</b>', '…', 16) AS body
              FROM search_index
              WHERE search_index MATCH ?
              ORDER BY rank LIMIT 100"
            .to_string(),
    };

    let mut q_builder = sqlx::query(&sql).bind(&match_expr);
    if let Some(ks) = kinds {
        for k in ks {
            q_builder = q_builder.bind(k);
        }
    }
    let rows = q_builder.fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(|r| SearchHit {
            entity_kind: r.get("entity_kind"),
            entity_id: r.get("entity_id"),
            name: r.get("name"),
            snippet: r.try_get::<String, _>("body").unwrap_or_default(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::build_match_expr;

    #[test]
    fn empty_input_yields_empty_expr() {
        assert_eq!(build_match_expr(""), "");
        assert_eq!(build_match_expr("   "), "");
        assert_eq!(build_match_expr(".-:"), "");
    }

    #[test]
    fn single_term_gets_prefix() {
        assert_eq!(build_match_expr("ben"), "\"ben\"*");
    }

    #[test]
    fn fts5_operators_are_neutralized() {
        // Bare AND/OR/NOT would be operators; quoting turns them into terms.
        assert_eq!(build_match_expr("AND"), "\"AND\"*");
    }

    #[test]
    fn punctuation_splits_into_separate_prefix_terms() {
        // Must mirror the unicode61 tokenizer, which split the indexed text on
        // these same boundaries — concatenating would match nothing.
        assert_eq!(build_match_expr("a.b"), "\"a\"* \"b\"*");
        assert_eq!(build_match_expr("foo:bar"), "\"foo\"* \"bar\"*");
        assert_eq!(build_match_expr("benchy.stl"), "\"benchy\"* \"stl\"*");
    }

    #[test]
    fn multiple_terms_join_with_space() {
        assert_eq!(build_match_expr("hull box"), "\"hull\"* \"box\"*");
    }
}
