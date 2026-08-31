use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

use crate::client::PanelClient;
use crate::session::load_session;

const BUILTIN_CATALOG: &str = include_str!("../../references/api-dev-v2.json");
const HTTP_METHODS: [&str; 5] = ["get", "post", "put", "delete", "patch"];

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    panel_version: String,
    route_count: usize,
    swagger_documented_count: usize,
    routes: Vec<Route>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Route {
    method: String,
    path: String,
    handler: String,
    source: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    swagger_documented: bool,
}

fn builtin() -> Result<Catalog> {
    serde_json::from_str(BUILTIN_CATALOG).map_err(|e| anyhow!("内置 API 清单损坏: {e}"))
}

fn client_for(profile: &str) -> Result<PanelClient> {
    let session = load_session(profile)?;
    PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)
}

fn runtime_swagger(profile: &str) -> Option<Value> {
    let client = client_for(profile).ok()?;
    let response = client.get("1panel/swagger/doc.json").ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json().ok()
}

fn normalize_path(path: &str) -> String {
    let path = path.trim();
    let path = path
        .strip_prefix("http://")
        .and_then(|p| p.split_once('/').map(|(_, rest)| rest))
        .unwrap_or(path);
    let path = path
        .strip_prefix("https://")
        .and_then(|p| p.split_once('/').map(|(_, rest)| rest))
        .unwrap_or(path);
    let path = path.trim_start_matches('/');
    if path == "api/v2" {
        "/api/v2".into()
    } else if path.starts_with("api/v2/") {
        format!("/{path}")
    } else {
        format!("/api/v2/{path}")
    }
}

fn swagger_operation<'a>(swagger: &'a Value, method: &str, full_path: &str) -> Option<&'a Value> {
    let swagger_path = full_path.strip_prefix("/api/v2").unwrap_or(full_path);
    swagger["paths"][swagger_path].get(method.to_ascii_lowercase())
}

fn route_text(route: &Route) -> String {
    format!(
        "{} {} {} {} {}",
        route.method,
        route.path,
        route.handler,
        route.summary,
        route.tags.join(" ")
    )
    .to_ascii_lowercase()
}

fn operation_tags(operation: &Value) -> Vec<String> {
    operation["tags"]
        .as_array()
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn merged_routes(catalog: &Catalog, swagger: Option<&Value>) -> Vec<Route> {
    let mut routes: BTreeMap<(String, String), Route> = catalog
        .routes
        .iter()
        .cloned()
        .map(|route| ((route.method.clone(), route.path.clone()), route))
        .collect();
    let Some(paths) = swagger.and_then(|root| root["paths"].as_object()) else {
        return routes.into_values().collect();
    };
    for (swagger_path, path_item) in paths {
        for method in HTTP_METHODS {
            let Some(operation) = path_item.get(method) else {
                continue;
            };
            let key = (method.to_ascii_uppercase(), normalize_path(swagger_path));
            if let Some(route) = routes.get_mut(&key) {
                route.swagger_documented = true;
                if route.summary.is_empty() {
                    route.summary = operation["summary"].as_str().unwrap_or("").to_string();
                }
                if route.tags.is_empty() {
                    route.tags = operation_tags(operation);
                }
            } else {
                routes.insert(
                    key.clone(),
                    Route {
                        method: key.0,
                        path: key.1,
                        handler: String::new(),
                        source: "runtime-swagger".into(),
                        summary: operation["summary"].as_str().unwrap_or("").to_string(),
                        tags: operation_tags(operation),
                        swagger_documented: true,
                    },
                );
            }
        }
    }
    routes.into_values().collect()
}

pub fn list(profile: &str, filter: &str) -> Result<()> {
    let catalog = builtin()?;
    let runtime = runtime_swagger(profile);
    let routes = merged_routes(&catalog, runtime.as_ref());
    let needle = filter.to_ascii_lowercase();
    let mut shown = 0usize;
    println!("{:<7} {:<58} 处理器 / 摘要", "方法", "路径");
    for route in &routes {
        if !needle.is_empty() && !route_text(route).contains(&needle) {
            continue;
        }
        let summary = if route.summary.is_empty() {
            &route.handler
        } else {
            &route.summary
        };
        println!("{:<7} {:<58} {}", route.method, route.path, summary);
        shown += 1;
    }
    println!(
        "显示 {shown}/{} 条路由;内置基线 1Panel {},Swagger 已记录 {}/{}",
        routes.len(),
        catalog.panel_version,
        catalog.swagger_documented_count,
        catalog.route_count
    );
    if runtime.is_none() {
        println!("提示:当前面板 Swagger 不可用,以上结果来自源码生成的内置完整清单。");
    } else if routes.len() > catalog.route_count {
        println!(
            "当前面板 Swagger 另补充 {} 条运行时路由。",
            routes.len() - catalog.route_count
        );
    }
    Ok(())
}

fn collect_refs(
    value: &Value,
    root: &Value,
    seen: &mut BTreeSet<String>,
    schemas: &mut Map<String, Value>,
) {
    match value {
        Value::Object(map) => {
            if let Some(reference) = map.get("$ref").and_then(Value::as_str) {
                if seen.insert(reference.to_string()) {
                    if let Some(pointer) = reference.strip_prefix('#') {
                        if let Some(schema) = root.pointer(pointer) {
                            let key = reference
                                .rsplit('/')
                                .next()
                                .unwrap_or(reference)
                                .to_string();
                            schemas.insert(key, schema.clone());
                            collect_refs(schema, root, seen, schemas);
                        }
                    }
                }
            }
            for child in map.values() {
                collect_refs(child, root, seen, schemas);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_refs(child, root, seen, schemas);
            }
        }
        _ => {}
    }
}

pub fn describe(profile: &str, method: &str, path: &str) -> Result<()> {
    let method = method.to_ascii_uppercase();
    let path = normalize_path(path);
    let catalog = builtin()?;
    let route = catalog
        .routes
        .iter()
        .find(|route| route.method == method && route.path == path);
    let runtime = runtime_swagger(profile);
    let operation = runtime
        .as_ref()
        .and_then(|swagger| swagger_operation(swagger, &method, &path));
    if route.is_none() && operation.is_none() {
        return Err(anyhow!(
            "内置 {} 清单和当前面板 Swagger 中均未找到 {method} {path};可运行 `api list --filter <关键字>`",
            catalog.panel_version
        ));
    }

    let mut result = BTreeMap::new();
    result.insert("method", json!(method));
    result.insert("path", json!(path));
    if let Some(route) = route {
        result.insert("handler", json!(route.handler));
        result.insert("source", json!(route.source));
        result.insert("summary", json!(route.summary));
        result.insert("tags", json!(route.tags));
        result.insert("swaggerDocumented", json!(route.swagger_documented));
    } else {
        result.insert("source", json!("runtime-swagger"));
        result.insert("runtimeOnly", json!(true));
    }

    if let (Some(swagger), Some(operation)) = (runtime.as_ref(), operation) {
        result.insert("operation", operation.clone());
        let mut seen = BTreeSet::new();
        let mut schemas = Map::new();
        collect_refs(operation, swagger, &mut seen, &mut schemas);
        if !schemas.is_empty() {
            result.insert("schemas", Value::Object(schemas));
        }
    } else if route.is_some() {
        result.insert(
            "note",
            json!("当前面板 Swagger 未收录或不可用;请求体请结合 handler/source 查看"),
        );
    }
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_is_valid_and_nonempty() {
        let catalog = builtin().unwrap();
        assert!(catalog.route_count > 300);
        assert_eq!(catalog.route_count, catalog.routes.len());
    }

    #[test]
    fn normalizes_api_paths() {
        assert_eq!(
            normalize_path("core/auth/current"),
            "/api/v2/core/auth/current"
        );
        assert_eq!(normalize_path("/api/v2/apps/search"), "/api/v2/apps/search");
    }

    #[test]
    fn merges_runtime_only_swagger_routes() {
        let catalog = builtin().unwrap();
        let swagger = json!({
            "paths": {
                "/future/feature": {
                    "post": {"summary": "Future feature", "tags": ["Future"]}
                }
            }
        });
        let routes = merged_routes(&catalog, Some(&swagger));
        assert_eq!(routes.len(), catalog.route_count + 1);
        assert!(routes
            .iter()
            .any(|route| route.path == "/api/v2/future/feature"));
    }
}
