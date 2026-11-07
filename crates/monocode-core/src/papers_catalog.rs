//! Open scholarly metadata, using the guarded network client shared with web.
use super::{reference_key, Paper, PaperMeta, PaperStore};
use serde_json::{json, Value};
use std::{collections::HashSet, time::Duration};
use url::Url;

fn string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn abstract_from_index(value: &Value) -> Option<String> {
    let mut words = vec![None; 20_000];
    for (word, positions) in value.as_object()? {
        for pos in positions.as_array().into_iter().flatten() {
            if let Some(slot) = pos.as_u64().and_then(|pos| words.get_mut(pos as usize)) {
                *slot = Some(word.as_str());
            }
        }
    }
    let text = words.into_iter().flatten().collect::<Vec<_>>().join(" ");
    (text.chars().count() >= 40).then_some(text)
}

fn parse_openalex(work: &Value) -> Option<Paper> {
    let title = string(work.get("title"))?;
    let doi = string(work.get("doi")).map(|value| reference_key(&value));
    let alex = string(work.get("id")).map(|id| id.rsplit('/').next().unwrap_or(&id).to_owned());
    let key = doi
        .clone()
        .or_else(|| alex.clone())
        .unwrap_or_else(|| title.to_lowercase());
    let primary = &work["primary_location"];
    let best = &work["best_oa_location"];
    let access = &work["open_access"];
    let mut urls = Vec::new();
    let mut add = |value: Option<String>| {
        if let Some(value) = value {
            if let Ok(url) = Url::parse(&value) {
                if matches!(url.scheme(), "http" | "https") && !urls.contains(&value) {
                    urls.push(value);
                }
            }
        }
    };
    add(string(best.get("pdf_url")));
    add(string(access.get("oa_url")));
    for location in work["locations"].as_array().into_iter().flatten() {
        add(string(location.get("pdf_url")));
        if location["is_oa"].as_bool() == Some(true) {
            add(string(location.get("landing_page_url")));
        }
    }
    add(string(best.get("landing_page_url")));
    let abstract_text = abstract_from_index(&work["abstract_inverted_index"]);
    Some(Paper {
        key,
        title,
        authors: work["authorships"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| string(entry["author"].get("display_name")))
            .collect(),
        pdf_urls: urls,
        access: match access["is_oa"].as_bool() {
            Some(true) => "open",
            Some(false) => "closed",
            None => "unknown",
        }
        .into(),
        doi,
        open_alex_id: alex,
        year: work["publication_year"]
            .as_u64()
            .and_then(|year| u32::try_from(year).ok()),
        venue: string(primary["source"].get("display_name")),
        cited_by: work["cited_by_count"].as_u64(),
        r#abstract: abstract_text.clone(),
        abstract_from: abstract_text.map(|_| "OpenAlex".into()),
        landing: string(primary.get("landing_page_url")),
    })
}

fn parse_crossref(item: &Value) -> Option<Paper> {
    let title = string(item["title"].as_array()?.first())?;
    let doi = string(item.get("DOI")).map(|value| reference_key(&value));
    let abstract_text = string(item.get("abstract")).and_then(|raw| {
        let plain = scraper::Html::parse_fragment(&raw)
            .root_element()
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        (plain.chars().count() >= 40).then_some(plain)
    });
    Some(Paper {
        key: doi.clone().unwrap_or_else(|| title.to_lowercase()),
        title,
        authors: item["author"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|author| {
                let name = [string(author.get("given")), string(author.get("family"))]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                (!name.is_empty()).then_some(name)
            })
            .collect(),
        pdf_urls: Vec::new(),
        access: "unknown".into(),
        doi: doi.clone(),
        open_alex_id: None,
        year: item["issued"]["date-parts"]
            .get(0)
            .and_then(|date| date.get(0))
            .and_then(Value::as_u64)
            .and_then(|year| u32::try_from(year).ok()),
        venue: string(item["container-title"].get(0)),
        cited_by: item["is-referenced-by-count"].as_u64(),
        r#abstract: abstract_text.clone(),
        abstract_from: abstract_text.map(|_| "Crossref".into()),
        landing: doi.map(|doi| format!("https://doi.org/{doi}")),
    })
}

fn fetch_json(url: Url) -> Result<Value, String> {
    if url.scheme() != "https"
        || !matches!(
            url.host_str(),
            Some("api.openalex.org" | "api.crossref.org")
        )
    {
        return Err("That is not a supported paper service.".into());
    }
    let response = crate::web::fetch_within(
        url,
        "application/json",
        4 * 1024 * 1024,
        Duration::from_secs(15),
    )?;
    serde_json::from_slice(&response.bytes)
        .map_err(|_| "The paper service did not answer with JSON.".into())
}

fn summary(paper: &Paper) -> Value {
    let mut value = json!({
        "ref": paper.doi.as_ref().or(paper.open_alex_id.as_ref()).unwrap_or(&paper.key),
        "title": paper.title,
        "authors": paper.authors.iter().take(4).collect::<Vec<_>>(),
        "access": paper.access,
    });
    if let Some(year) = paper.year {
        value["year"] = json!(year);
    }
    if let Some(venue) = &paper.venue {
        value["venue"] = json!(venue);
    }
    if let Some(count) = paper.cited_by {
        value["citedBy"] = json!(count);
    }
    if let Some(text) = &paper.r#abstract {
        value["abstractStart"] = json!(text.chars().take(220).collect::<String>());
    }
    if let Some(doi) = &paper.doi {
        value["link"] = json!(format!("https://doi.org/{doi}"));
    }
    value
}

/// Parse and validate before invoking a caller-provided metadata service.
pub fn find(
    store: &PaperStore,
    input: &Value,
    mut fetch: impl FnMut(Url) -> Result<Value, String>,
) -> Result<Value, String> {
    let query = crate::web::required_string(&input["query"], "query", 300)?;
    let limit = input
        .get("limit")
        .map_or(Some(6), Value::as_u64)
        .filter(|limit| (1..=10).contains(limit))
        .ok_or("limit must be an integer from 1 to 10")?;
    let year = match input.get("fromYear") {
        None => None,
        Some(year) => Some(
            year.as_u64()
                .filter(|year| (1900..=2100).contains(year))
                .ok_or("fromYear must be a year")?,
        ),
    };
    let mut url = Url::parse("https://api.openalex.org/works").unwrap();
    {
        let mut params = url.query_pairs_mut();
        params.append_pair("search", &query);
        params.append_pair("per-page", &limit.to_string());
        params.append_pair("select", "id,doi,title,publication_year,cited_by_count,authorships,primary_location,best_oa_location,open_access,abstract_inverted_index,locations");
        if let Some(year) = year {
            params.append_pair("filter", &format!("from_publication_date:{year}-01-01"));
        }
    }
    let openalex = fetch(url)
        .ok()
        .and_then(|response| response["results"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|work| parse_openalex(&work))
        .collect::<Vec<_>>();
    let (source, results) = if openalex.is_empty() {
        let mut url = Url::parse("https://api.crossref.org/works").unwrap();
        {
            let mut params = url.query_pairs_mut();
            params.append_pair("query.bibliographic", &query);
            params.append_pair("rows", &limit.to_string());
            if let Some(year) = year {
                params.append_pair("filter", &format!("from-pub-date:{year}"));
            }
        }
        let response = fetch(url)?;
        let items = response["message"]["items"]
            .as_array()
            .ok_or("The paper service returned invalid results.")?;
        (
            "Crossref",
            items.iter().filter_map(parse_crossref).collect::<Vec<_>>(),
        )
    } else {
        ("OpenAlex", openalex)
    };
    let mut seen = HashSet::new();
    let mut papers = Vec::new();
    for paper in results {
        if seen.insert(paper.key.to_lowercase()) {
            store.remember(paper.clone())?;
            papers.push(summary(&paper));
            if papers.len() == limit as usize {
                break;
            }
        }
    }
    let next = if papers.is_empty() {
        "Nothing matched: try other words."
    } else {
        "Call papers.read with a paper's ref to get its abstract and, when available, its text."
    };
    let mut result = json!({"source":source,"papers":papers,"next":next});
    if source == "Crossref" {
        result["note"] = json!("OpenAlex was unavailable or returned no results; Crossref may have less information about open copies.");
    }
    Ok(result)
}

pub fn find_input(store: &PaperStore, input: &Value) -> Result<Value, String> {
    find(store, input, fetch_json)
}

/// Resolve a cached paper first; a DOI or OpenAlex id can also be looked up
/// without a preceding search. Never look up locally fingerprinted PDFs online.
pub fn resolve(store: &PaperStore, reference: &str) -> Result<PaperMeta, String> {
    resolve_with(store, reference, fetch_json)
}

fn resolve_with(
    store: &PaperStore,
    reference: &str,
    mut fetch: impl FnMut(Url) -> Result<Value, String>,
) -> Result<PaperMeta, String> {
    let key = reference_key(reference);
    if let Some(meta) = store.meta(&key)? {
        return Ok(meta);
    }
    if key.starts_with("pdf:") {
        return Err("This paper's PDF is not in the cache.".into());
    }
    let mut url = Url::parse("https://api.openalex.org").unwrap();
    if key.starts_with("10.") && key.contains('/') {
        url.set_path(&format!("/works/doi:{key}"));
    } else if key.len() > 1 && key.starts_with('w') && key[1..].chars().all(|c| c.is_ascii_digit())
    {
        url.set_path(&format!("/works/{}", key.to_ascii_uppercase()));
    } else {
        let result = find(store, &json!({"query": reference, "limit": 1}), &mut fetch)?;
        let found = result["papers"][0]["ref"]
            .as_str()
            .ok_or("No paper was found for that reference.")?;
        return store
            .meta(&reference_key(found))?
            .ok_or_else(|| "No paper was found for that reference.".into());
    }
    let paper = fetch(url)
        .ok()
        .and_then(|response| parse_openalex(&response));
    let paper = if let Some(paper) = paper {
        paper
    } else if key.starts_with("10.") && key.contains('/') {
        let mut url = Url::parse("https://api.crossref.org").unwrap();
        url.set_path(&format!("/works/{key}"));
        let response = fetch(url)?;
        parse_crossref(&response["message"]).ok_or("No paper was found for that reference.")?
    } else {
        return Err("No paper was found for that reference.".into());
    };
    store.remember(paper)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_maps_metadata_and_reuses_a_cached_full_paper() {
        let dir = tempfile::tempdir().unwrap();
        let store = PaperStore::new(dir.path().to_path_buf());
        let result = find(&store, &json!({"query":"river models","limit":2,"fromYear":2020}), |url| {
            assert_eq!(url.host_str(), Some("api.openalex.org"));
            assert!(url.query().unwrap().contains("river+models"));
            Ok(json!({"results":[{
                "title":"A model",
                "doi":"https://doi.org/10.1234/Test",
                "id":"https://openalex.org/W12345",
                "publication_year":2022,
                "authorships":[{"author":{"display_name":"A Researcher"}}],
                "open_access":{"is_oa":true},
                "best_oa_location":{"pdf_url":"https://example.org/file.pdf"},
                "abstract_inverted_index":{"A":[0],"long":[1],"abstract":[2],"with":[3],"enough":[4],"content":[5],"for":[6],"a":[7],"scientific":[8],"paper.":[9]}
            }]}))
        }).unwrap();
        assert_eq!(result["papers"][0]["ref"], "10.1234/test");
        let meta = store.meta("10.1234/test").unwrap().unwrap();
        assert_eq!(meta.paper.pdf_urls, ["https://example.org/file.pdf"]);
        assert!(meta.paper.r#abstract.is_some());
        assert!(find(
            &store,
            &json!({"query":"x","limit":100}),
            |_| unreachable!()
        )
        .is_err());
    }

    #[test]
    fn resolves_dois_without_search_and_does_not_query_for_missing_local_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let store = PaperStore::new(dir.path().to_path_buf());
        let meta = resolve_with(&store, "https://doi.org/10.1234/Test", |url| {
            assert!(url.path().contains("/works/doi:10.1234/test"));
            Ok(json!({"title":"Study","doi":"https://doi.org/10.1234/test"}))
        })
        .unwrap();
        assert_eq!(meta.paper.key, "10.1234/test");
        assert!(resolve_with(&store, "10.1234/test", |_| unreachable!()).is_ok());
        assert!(resolve_with(&store, "pdf:deadbeef", |_| unreachable!()).is_err());
    }

    #[test]
    fn crossref_is_used_when_openalex_fails_for_search_and_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let store = PaperStore::new(dir.path().to_path_buf());
        let item = json!({
            "DOI":"10.1234/fallback",
            "title":["A hydrology study"],
            "author":[{"given":"Ada","family":"Researcher"}],
            "issued":{"date-parts":[[2021]]},
            "abstract":"<jats:p>This paper describes a substantial study of watershed hydrology and measured precipitation.</jats:p>"
        });
        let result = find(&store, &json!({"query":"hydrology"}), |url| {
            if url.host_str() == Some("api.openalex.org") {
                Err("rate limited".into())
            } else {
                Ok(json!({"message":{"items":[item.clone()]}}))
            }
        })
        .unwrap();
        assert_eq!(result["source"], "Crossref");
        assert_eq!(result["papers"][0]["ref"], "10.1234/fallback");
        assert_eq!(
            store
                .meta("10.1234/fallback")
                .unwrap()
                .unwrap()
                .paper
                .authors,
            ["Ada Researcher"]
        );
        let other = tempfile::tempdir().unwrap();
        let lookup = PaperStore::new(other.path().to_path_buf());
        let meta = resolve_with(&lookup, "10.1234/fallback", |url| {
            if url.host_str() == Some("api.openalex.org") {
                Err("unavailable".into())
            } else {
                Ok(json!({"message":item.clone()}))
            }
        })
        .unwrap();
        assert_eq!(meta.availability, "abstract-only");
        assert!(meta
            .paper
            .r#abstract
            .unwrap()
            .contains("watershed hydrology"));
        assert!(meta.paper.pdf_urls.is_empty());
    }
}
