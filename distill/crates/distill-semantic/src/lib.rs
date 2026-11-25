use distill_proto::{SemanticCard, SemanticKind};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, Class, ClassElement, Declaration, Decorator, Expression, ObjectPropertyKind,
    PropertyKey, Statement, TSType,
};
use oxc_parser::Parser;
use oxc_span::SourceType;
use pulldown_cmark::{Event, HeadingLevel, Parser as MdParser, Tag, TagEnd};
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Extract sanitized semantic cards from a single repository file.
/// Raw function bodies and implementation lines are deliberately excluded.
pub fn extract_file_semantics(rel_path: &str, content: &str) -> Vec<SemanticCard> {
    if rel_path.ends_with(".md") {
        return extract_markdown(rel_path, content);
    }
    if rel_path.ends_with(".json") {
        return extract_json(rel_path, content);
    }
    if rel_path.ends_with(".ts") || rel_path.ends_with(".tsx") {
        return extract_typescript(rel_path, content);
    }
    Vec::new()
}

fn infer_domain(rel_path: &str) -> String {
    let parts: Vec<&str> = rel_path.split('/').collect();
    if parts.len() >= 4 && parts[0] == "apps" && parts[2] == "src" {
        return format!("{}/{}", parts[1], parts[3]);
    }
    if parts.len() >= 2 && parts[0] == "packages" {
        return format!("pkg/{}", parts[1]);
    }
    if parts.len() >= 2 && parts[0] == "docs" {
        return format!("docs/{}", parts[1]);
    }
    parts.first().copied().unwrap_or("root").to_string()
}

fn stable_card_id(rel_path: &str, kind: SemanticKind, anchor: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(rel_path.as_bytes());
    h.update(b"|");
    h.update(kind.as_str().as_bytes());
    h.update(b"|");
    h.update(anchor.as_bytes());
    let hex = h.finalize().to_hex();
    format!("{}:{}", kind.as_str(), &hex[..16])
}

// ============================================================================
// TypeScript / NestJS / TypeORM / Migration Pure Rust AST Extractor (oxc)
// ============================================================================

static RE_FIELD_SPEC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"key\s*:\s*['"]([A-Za-z0-9_.\-]+)['"]\s*,\s*label\s*:\s*['"]([^'"]+)['"](?:\s*,\s*fieldType\s*:\s*['"]([A-Z_]+)['"])?"#).unwrap()
});
static RE_YUNXIAO_SPEC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"key\s*:\s*['"]([A-Za-z0-9_.\-]+)['"]\s*,\s*fieldId\s*:\s*['"]([A-Za-z0-9_.\-]+)['"]\s*,\s*fieldName\s*:\s*['"]([^'"]+)['"]"#).unwrap()
});
static RE_PLATFORM_EX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"createPlatformException\s*\(\s*(\d{3})\s*,\s*\{[^}]{0,400}?errorCode\s*:\s*['"]([A-Z0-9_]+)['"][^}]{0,400}?messageKey\s*:\s*['"]([^'"]+)['"]"#).unwrap()
});
static RE_CREATE_TABLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?["`]?([a-zA-Z0-9_]+)["`]?"#).unwrap());

fn extract_typescript(rel_path: &str, content: &str) -> Vec<SemanticCard> {
    let mut cards = Vec::new();
    let domain = infer_domain(rel_path);
    let source_type = SourceType::from_path(rel_path).unwrap_or_else(|_| SourceType::ts());
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, content, source_type).parse();

    for stmt in &parsed.program.body {
        match stmt {
            Statement::ExportNamedDeclaration(export) => {
                if let Some(decl) = &export.declaration {
                    extract_declaration(rel_path, &domain, decl, &mut cards);
                }
            }
            Statement::ClassDeclaration(class) => {
                extract_class(rel_path, &domain, class, &mut cards);
            }
            Statement::VariableDeclaration(var) => {
                extract_const_catalog(rel_path, &domain, var, &mut cards);
            }
            _ => {}
        }
    }

    // Extract field specs (e.g. migration fieldSpecs like qaValuePointRound1 -> 价值点（集测1轮）)
    for cap in RE_FIELD_SPEC.captures_iter(content) {
        let key = cap[1].to_string();
        let label = cap[2].to_string();
        let ftype = cap.get(3).map(|m| m.as_str()).unwrap_or("UNKNOWN");
        let kind = if rel_path.contains("/migrations/") {
            SemanticKind::MigrationFact
        } else {
            SemanticKind::ConstantCatalog
        };
        let mut meta = BTreeMap::new();
        meta.insert("field_key".into(), key.clone());
        meta.insert("label".into(), label.clone());
        meta.insert("field_type".into(), ftype.to_string());
        cards.push(SemanticCard {
            card_id: stable_card_id(rel_path, kind, &key),
            kind,
            domain: domain.clone(),
            title: format!("字段定义：{label} ({key})"),
            summary: format!("工作项扩展字段「{label}」对应的内部稳定 key 为 `{key}`，字段类型 `{ftype}`。"),
            metadata: meta,
            tags: vec![domain.clone(), key, label, ftype.to_lowercase()],
        });
    }

    // Extract Yunxiao fixed field mappings (fieldId <-> key <-> fieldName)
    for cap in RE_YUNXIAO_SPEC.captures_iter(content) {
        let key = cap[1].to_string();
        let field_id = cap[2].to_string();
        let field_name = cap[3].to_string();
        let mut meta = BTreeMap::new();
        meta.insert("field_key".into(), key.clone());
        meta.insert("yunxiao_field_id".into(), field_id.clone());
        meta.insert("field_name".into(), field_name.clone());
        cards.push(SemanticCard {
            card_id: stable_card_id(rel_path, SemanticKind::ConstantCatalog, &format!("yx:{key}")),
            kind: SemanticKind::ConstantCatalog,
            domain: domain.clone(),
            title: format!("兼容字段映射：{field_name} ({key} / {field_id})"),
            summary: format!(
                "业务字段「{field_name}」的内部 key 为 `{key}`，云效兼容接口固定 fieldId 为 `{field_id}`。"
            ),
            metadata: meta,
            tags: vec![domain.clone(), key, field_id, field_name],
        });
    }

    // Extract createPlatformException error contracts
    for cap in RE_PLATFORM_EX.captures_iter(content) {
        let status = cap[1].to_string();
        let error_code = cap[2].to_string();
        let message_key = cap[3].to_string();
        let mut meta = BTreeMap::new();
        meta.insert("http_status".into(), status.clone());
        meta.insert("error_code".into(), error_code.clone());
        meta.insert("message_key".into(), message_key.clone());
        cards.push(SemanticCard {
            card_id: stable_card_id(rel_path, SemanticKind::ErrorContract, &error_code),
            kind: SemanticKind::ErrorContract,
            domain: domain.clone(),
            title: format!("业务异常 {error_code} (HTTP {status})"),
            summary: format!(
                "接口在域 `{domain}` 中可能返回 HTTP {status}，错误码 `{error_code}`，对应共享双语文案键 `{message_key}`。"
            ),
            metadata: meta,
            tags: vec![domain.clone(), error_code, message_key],
        });
    }

    // Extract migration table creation facts
    if rel_path.contains("/migrations/") {
        let tables: Vec<String> = RE_CREATE_TABLE
            .captures_iter(content)
            .map(|c| c[1].to_string())
            .collect();
        if !tables.is_empty() {
            let mig_name = rel_path.rsplit('/').next().unwrap_or(rel_path);
            let mut meta = BTreeMap::new();
            meta.insert("tables".into(), tables.join(", "));
            cards.push(SemanticCard {
                card_id: stable_card_id(rel_path, SemanticKind::MigrationFact, mig_name),
                kind: SemanticKind::MigrationFact,
                domain: domain.clone(),
                title: format!("数据库迁移 {mig_name}"),
                summary: format!("该增量迁移新建数据表：{}。", tables.join("、")),
                metadata: meta,
                tags: vec!["migration".into(), domain.clone()],
            });
        }
    }

    cards
}

fn extract_declaration(
    rel_path: &str,
    domain: &str,
    decl: &Declaration<'_>,
    cards: &mut Vec<SemanticCard>,
) {
    match decl {
        Declaration::ClassDeclaration(class) => extract_class(rel_path, domain, class, cards),
        Declaration::VariableDeclaration(var) => extract_const_catalog(rel_path, domain, var, cards),
        Declaration::TSTypeAliasDeclaration(ta) => {
            let name = ta.id.name.as_str();
            if name.ends_with("Request") || name.ends_with("Result") || name.ends_with("Payload") {
                cards.push(SemanticCard {
                    card_id: stable_card_id(rel_path, SemanticKind::DtoSchema, name),
                    kind: SemanticKind::DtoSchema,
                    domain: domain.to_string(),
                    title: format!("契约类型 {name}"),
                    summary: format!("领域 `{domain}` 导出的共享契约类型 `{name}`。"),
                    metadata: BTreeMap::from([("type_name".into(), name.to_string())]),
                    tags: vec![domain.to_string(), name.to_string()],
                });
            }
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            let name = iface.id.name.as_str();
            let fields: Vec<String> = iface
                .body
                .body
                .iter()
                .filter_map(|sig| match sig {
                    oxc_ast::ast::TSSignature::TSPropertySignature(prop) => {
                        let key = property_key_name(&prop.key)?;
                        let opt = if prop.optional { "?" } else { "" };
                        let ty = prop
                            .type_annotation
                            .as_ref()
                            .map(|t| summarize_ts_type(&t.type_annotation))
                            .unwrap_or_else(|| "unknown".into());
                        Some(format!("{key}{opt}: {ty}"))
                    }
                    _ => None,
                })
                .collect();
            if !fields.is_empty() && fields.len() <= 40 {
                let mut meta = BTreeMap::new();
                meta.insert("interface".into(), name.to_string());
                meta.insert("fields".into(), fields.join("; "));
                cards.push(SemanticCard {
                    card_id: stable_card_id(rel_path, SemanticKind::DtoSchema, name),
                    kind: SemanticKind::DtoSchema,
                    domain: domain.to_string(),
                    title: format!("接口数据结构 {name}"),
                    summary: format!("`{name}` 包含字段：{}。", fields.join("，")),
                    metadata: meta,
                    tags: vec![domain.to_string(), name.to_string()],
                });
            }
        }
        _ => {}
    }
}

fn extract_class(
    rel_path: &str,
    domain: &str,
    class: &Class<'_>,
    cards: &mut Vec<SemanticCard>,
) {
    let class_name = class
        .id
        .as_ref()
        .map(|id| id.name.as_str())
        .unwrap_or("AnonymousClass");

    let ctrl_prefix = find_decorator_first_string(&class.decorators, "Controller");
    let api_tags = find_decorator_first_string(&class.decorators, "ApiTags");
    let api_header = find_decorator_object_prop(&class.decorators, "ApiHeader", "name");
    let entity_table = find_decorator_object_prop(&class.decorators, "Entity", "name")
        .or_else(|| find_decorator_first_string(&class.decorators, "Entity"));

    // 1) NestJS Controller endpoints
    if let Some(prefix) = ctrl_prefix {
        for element in &class.body.body {
            if let ClassElement::MethodDefinition(method) = element {
                let method_name = property_key_name(&method.key).unwrap_or_default();
                let http_route = find_http_method_decorator(&method.decorators);
                let Some((http_verb, sub_path)) = http_route else {
                    continue;
                };
                let full_path = join_route("/api/v1", &prefix, &sub_path);
                let summary_text =
                    find_decorator_object_prop(&method.decorators, "ApiOperation", "summary")
                        .unwrap_or_else(|| format!("{class_name}.{method_name}"));
                let is_public = has_decorator(&method.decorators, "Public");
                let perms = find_decorator_all_args(&method.decorators, "RequireLocalPermissions");

                // Extract parameter DTO types without touching method body
                let mut body_dto = None;
                let mut query_params = Vec::new();
                let mut path_params = Vec::new();
                for param in &method.value.params.items {
                    let type_name = param
                        .type_annotation
                        .as_ref()
                        .map(|t| summarize_ts_type(&t.type_annotation));
                    if has_decorator(&param.decorators, "Body") {
                        body_dto = type_name;
                    } else if let Some(q) = find_decorator_first_string(&param.decorators, "Query") {
                        query_params.push(q);
                    } else if let Some(p) = find_decorator_first_string(&param.decorators, "Param") {
                        path_params.push(p);
                    }
                }

                let mut meta = BTreeMap::new();
                meta.insert("method".into(), http_verb.to_string());
                meta.insert("path".into(), full_path.clone());
                meta.insert("operation".into(), format!("{class_name}_{method_name}"));
                if let Some(t) = &api_tags {
                    meta.insert("swagger_tag".into(), t.clone());
                }
                if let Some(h) = &api_header {
                    meta.insert("auth_header".into(), h.clone());
                } else if !is_public {
                    meta.insert("auth_header".into(), "Authorization: Bearer <token>".into());
                }
                if !perms.is_empty() {
                    meta.insert("permissions".into(), perms.join(", "));
                }
                if let Some(dto) = &body_dto {
                    meta.insert("body_dto".into(), dto.clone());
                }
                if !path_params.is_empty() {
                    meta.insert("path_params".into(), path_params.join(", "));
                }
                if !query_params.is_empty() {
                    meta.insert("query_params".into(), query_params.join(", "));
                }

                let auth_desc = if let Some(h) = &api_header {
                    format!("鉴权头 `{h}`")
                } else if is_public {
                    "独立外部/公开鉴权".to_string()
                } else {
                    format!(
                        "Bearer JWT 鉴权{}",
                        if perms.is_empty() {
                            String::new()
                        } else {
                            format!("（所需权限：{}）", perms.join(", "))
                        }
                    )
                };

                let dto_desc = body_dto
                    .as_ref()
                    .map(|d| format!("，请求体类型 `{d}`"))
                    .unwrap_or_default();

                cards.push(SemanticCard {
                    card_id: stable_card_id(
                        rel_path,
                        SemanticKind::ApiEndpoint,
                        &format!("{http_verb}:{full_path}"),
                    ),
                    kind: SemanticKind::ApiEndpoint,
                    domain: domain.to_string(),
                    title: format!("{http_verb} {full_path} — {summary_text}"),
                    summary: format!(
                        "{summary_text}。接口分组「{}」，{auth_desc}{dto_desc}。",
                        api_tags.as_deref().unwrap_or(domain)
                    ),
                    metadata: meta,
                    tags: vec![
                        domain.to_string(),
                        http_verb.to_lowercase(),
                        full_path,
                        class_name.to_string(),
                        method_name,
                    ],
                });
            }
        }
        return;
    }

    // 2) TypeORM Entity schema
    if let Some(table) = entity_table {
        let mut columns = Vec::new();
        for element in &class.body.body {
            if let ClassElement::PropertyDefinition(prop) = element {
                if let Some(pname) = property_key_name(&prop.key) {
                    let ty = prop
                        .type_annotation
                        .as_ref()
                        .map(|t| summarize_ts_type(&t.type_annotation))
                        .unwrap_or_else(|| "unknown".into());
                    columns.push(format!("{pname}: {ty}"));
                }
            }
        }
        let mut meta = BTreeMap::new();
        meta.insert("entity".into(), class_name.to_string());
        meta.insert("table".into(), table.clone());
        meta.insert("columns".into(), columns.join("; "));
        cards.push(SemanticCard {
            card_id: stable_card_id(rel_path, SemanticKind::EntitySchema, class_name),
            kind: SemanticKind::EntitySchema,
            domain: domain.to_string(),
            title: format!("数据表实体 {class_name} (table: {table})"),
            summary: format!("PostgreSQL 表 `{table}` 对应实体 `{class_name}`，字段：{}。", columns.join("，")),
            metadata: meta,
            tags: vec![domain.to_string(), table, class_name.to_string()],
        });
        return;
    }

    // 3) DTO classes
    if class_name.ends_with("Dto") || class_name.ends_with("Params") {
        let mut fields = Vec::new();
        for element in &class.body.body {
            if let ClassElement::PropertyDefinition(prop) = element {
                if let Some(pname) = property_key_name(&prop.key) {
                    let opt = if prop.optional || has_decorator(&prop.decorators, "IsOptional") {
                        "?"
                    } else {
                        ""
                    };
                    let ty = prop
                        .type_annotation
                        .as_ref()
                        .map(|t| summarize_ts_type(&t.type_annotation))
                        .unwrap_or_else(|| "unknown".into());
                    let decs: Vec<String> = prop
                        .decorators
                        .iter()
                        .filter_map(decorator_name)
                        .filter(|n| n.starts_with("Is") || n.starts_with("Min") || n.starts_with("Max") || n == "Length" || n == "ArrayMaxSize")
                        .collect();
                    if decs.is_empty() {
                        fields.push(format!("{pname}{opt}: {ty}"));
                    } else {
                        fields.push(format!("{pname}{opt}: {ty} [{}]", decs.join(",")));
                    }
                }
            }
        }
        if !fields.is_empty() {
            let mut meta = BTreeMap::new();
            meta.insert("dto".into(), class_name.to_string());
            meta.insert("fields".into(), fields.join("; "));
            cards.push(SemanticCard {
                card_id: stable_card_id(rel_path, SemanticKind::DtoSchema, class_name),
                kind: SemanticKind::DtoSchema,
                domain: domain.to_string(),
                title: format!("请求参数 DTO：{class_name}"),
                summary: format!("`{class_name}` 定义的入参字段与校验约束：{}。", fields.join("；")),
                metadata: meta,
                tags: vec![domain.to_string(), class_name.to_string(), "dto".into()],
            });
        }
    }
}

fn extract_const_catalog(
    rel_path: &str,
    domain: &str,
    var: &oxc_ast::ast::VariableDeclaration<'_>,
    cards: &mut Vec<SemanticCard>,
) {
    for decl in &var.declarations {
        let Some(name) = decl.id.get_identifier_name() else {
            continue;
        };
        let name_str = name.as_str();
        let interesting = name_str.contains("FieldKey")
            || name_str.contains("CustomField")
            || name_str.contains("Permission")
            || name_str.contains("OptionIds")
            || name_str.contains("Statuses")
            || name_str.contains("Priorities");
        if !interesting {
            continue;
        }
        let Some(init) = &decl.init else { continue };
        let summary_val = summarize_const_expr(init);
        if summary_val.is_empty() {
            continue;
        }
        let mut meta = BTreeMap::new();
        meta.insert("symbol".into(), name_str.to_string());
        meta.insert("values".into(), summary_val.clone());
        cards.push(SemanticCard {
            card_id: stable_card_id(rel_path, SemanticKind::ConstantCatalog, name_str),
            kind: SemanticKind::ConstantCatalog,
            domain: domain.to_string(),
            title: format!("稳定常量目录：{name_str}"),
            summary: format!("域 `{domain}` 定义的稳定键值/枚举 `{name_str}`：{summary_val}"),
            metadata: meta,
            tags: vec![domain.to_string(), name_str.to_string(), "constant".into()],
        });
    }
}

fn summarize_const_expr(expr: &Expression<'_>) -> String {
    match expr {
        Expression::StringLiteral(s) => s.value.to_string(),
        Expression::TSAsExpression(a) => summarize_const_expr(&a.expression),
        Expression::TSSatisfiesExpression(s) => summarize_const_expr(&s.expression),
        Expression::ObjectExpression(obj) => {
            let pairs: Vec<String> = obj
                .properties
                .iter()
                .filter_map(|p| match p {
                    ObjectPropertyKind::ObjectProperty(prop) => {
                        let k = property_key_name(&prop.key)?;
                        let v = summarize_const_expr(&prop.value);
                        if v.is_empty() {
                            None
                        } else {
                            Some(format!("{k}={v}"))
                        }
                    }
                    _ => None,
                })
                .take(30)
                .collect();
            pairs.join(", ")
        }
        Expression::ArrayExpression(arr) => {
            let items: Vec<String> = arr
                .elements
                .iter()
                .filter_map(|el| el.as_expression().map(summarize_const_expr))
                .filter(|s| !s.is_empty())
                .take(30)
                .collect();
            items.join(", ")
        }
        _ => String::new(),
    }
}

// ============================================================================
// JSON Semantic Extractor (error messages, contracts/generated)
// ============================================================================

fn extract_json(rel_path: &str, content: &str) -> Vec<SemanticCard> {
    let mut cards = Vec::new();
    let domain = infer_domain(rel_path);

    if rel_path.contains("error-messages/") && rel_path.ends_with(".json") {
        let locale = if rel_path.contains("zh-CN") {
            "zh-CN"
        } else if rel_path.contains("en-US") {
            "en-US"
        } else {
            "default"
        };
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(content) {
            for (key, val) in map {
                if let Some(msg) = val.as_str() {
                    let mut meta = BTreeMap::new();
                    meta.insert("message_key".into(), key.clone());
                    meta.insert("locale".into(), locale.to_string());
                    cards.push(SemanticCard {
                        card_id: stable_card_id(
                            rel_path,
                            SemanticKind::ErrorContract,
                            &format!("{locale}:{key}"),
                        ),
                        kind: SemanticKind::ErrorContract,
                        domain: domain.clone(),
                        title: format!("错误文案 [{locale}] {key}"),
                        summary: format!("错误键 `{key}`（{locale}）：{msg}"),
                        metadata: meta,
                        tags: vec!["error-message".into(), locale.into(), key],
                    });
                }
            }
        }
        return cards;
    }

    if rel_path.starts_with("contracts/generated/") && rel_path.ends_with(".json") {
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(content) {
            let keys: Vec<String> = map.keys().take(25).cloned().collect();
            let fname = rel_path.rsplit('/').next().unwrap_or(rel_path);
            let mut meta = BTreeMap::new();
            meta.insert("artifact".into(), fname.to_string());
            meta.insert("top_keys".into(), keys.join(", "));
            cards.push(SemanticCard {
                card_id: stable_card_id(rel_path, SemanticKind::ConstantCatalog, fname),
                kind: SemanticKind::ConstantCatalog,
                domain,
                title: format!("共享内核生成合同：{fname}"),
                summary: format!("生成制品 `{fname}` 包含顶层语义项：{}。", keys.join("、")),
                metadata: meta,
                tags: vec!["shared-kernel".into(), fname.to_string()],
            });
        }
    }

    cards
}

// ============================================================================
// Markdown Semantic Extractor (strips code blocks, indexes business sections)
// ============================================================================

fn extract_markdown(rel_path: &str, content: &str) -> Vec<SemanticCard> {
    let mut cards = Vec::new();
    let domain = infer_domain(rel_path);
    let doc_name = rel_path.rsplit('/').next().unwrap_or(rel_path);

    let mut current_heading = doc_name.to_string();
    let mut current_body = String::new();
    let mut in_heading = false;
    let mut heading_buf = String::new();
    let mut in_code_block = false;
    let mut section_idx = 0usize;

    let flush = |heading: &str,
                 body: &str,
                 idx: &mut usize,
                 out: &mut Vec<SemanticCard>| {
        let cleaned = body.split_whitespace().collect::<Vec<_>>().join(" ");
        if cleaned.len() < 16 {
            return;
        }
        *idx += 1;
        let truncated: String = cleaned.chars().take(1200).collect();
        let mut meta = BTreeMap::new();
        meta.insert("doc".into(), doc_name.to_string());
        meta.insert("section".into(), heading.to_string());
        out.push(SemanticCard {
            card_id: stable_card_id(rel_path, SemanticKind::DocSection, &format!("{idx}:{heading}")),
            kind: SemanticKind::DocSection,
            domain: domain.clone(),
            title: format!("{doc_name} / {heading}"),
            summary: truncated,
            metadata: meta,
            tags: vec!["doc".into(), domain.clone(), doc_name.to_string()],
        });
    };

    for event in MdParser::new(content) {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1 | HeadingLevel::H2 | HeadingLevel::H3,
                ..
            }) => {
                flush(&current_heading, &current_body, &mut section_idx, &mut cards);
                current_body.clear();
                heading_buf.clear();
                in_heading = true;
            }
            Event::End(TagEnd::Heading(
                HeadingLevel::H1 | HeadingLevel::H2 | HeadingLevel::H3,
            )) => {
                in_heading = false;
                if !heading_buf.trim().is_empty() {
                    current_heading = heading_buf.trim().to_string();
                }
            }
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
            }
            Event::Text(t) | Event::Code(t) => {
                if in_code_block {
                    continue;
                }
                if in_heading {
                    heading_buf.push_str(&t);
                } else {
                    current_body.push_str(&t);
                    current_body.push(' ');
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if !in_heading && !in_code_block {
                    current_body.push(' ');
                }
            }
            _ => {}
        }
    }
    flush(&current_heading, &current_body, &mut section_idx, &mut cards);
    cards
}

// ============================================================================
// AST Helper Functions
// ============================================================================

fn decorator_name(dec: &Decorator<'_>) -> Option<String> {
    match &dec.expression {
        Expression::Identifier(id) => Some(id.name.to_string()),
        Expression::CallExpression(call) => match &call.callee {
            Expression::Identifier(id) => Some(id.name.to_string()),
            _ => None,
        },
        _ => None,
    }
}

fn has_decorator(decs: &[Decorator<'_>], target: &str) -> bool {
    decs.iter()
        .any(|d| decorator_name(d).as_deref() == Some(target))
}

fn find_decorator_first_string(decs: &[Decorator<'_>], target: &str) -> Option<String> {
    for d in decs {
        if let Expression::CallExpression(call) = &d.expression
            && let Expression::Identifier(id) = &call.callee
            && id.name == target
        {
            if let Some(Argument::StringLiteral(s)) = call.arguments.first() {
                return Some(s.value.to_string());
            }
            return Some(String::new());
        }
    }
    None
}

fn find_decorator_all_args(decs: &[Decorator<'_>], target: &str) -> Vec<String> {
    let mut out = Vec::new();
    for d in decs {
        if let Expression::CallExpression(call) = &d.expression
            && let Expression::Identifier(id) = &call.callee
            && id.name == target
        {
            for arg in &call.arguments {
                match arg {
                    Argument::StringLiteral(s) => out.push(s.value.to_string()),
                    Argument::StaticMemberExpression(m) => {
                        out.push(m.property.name.to_string());
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

fn find_decorator_object_prop(
    decs: &[Decorator<'_>],
    target: &str,
    prop_name: &str,
) -> Option<String> {
    for d in decs {
        if let Expression::CallExpression(call) = &d.expression
            && let Expression::Identifier(id) = &call.callee
            && id.name == target
            && let Some(Argument::ObjectExpression(obj)) = call.arguments.first()
        {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop
                    && property_key_name(&p.key).as_deref() == Some(prop_name)
                    && let Expression::StringLiteral(s) = &p.value
                {
                    return Some(s.value.to_string());
                }
            }
        }
    }
    None
}

fn find_http_method_decorator(decs: &[Decorator<'_>]) -> Option<(&'static str, String)> {
    const VERBS: [(&str, &str); 5] = [
        ("Get", "GET"),
        ("Post", "POST"),
        ("Patch", "PATCH"),
        ("Put", "PUT"),
        ("Delete", "DELETE"),
    ];
    for (dec_name, verb) in VERBS {
        if let Some(path) = find_decorator_first_string(decs, dec_name) {
            return Some((verb, path));
        }
    }
    None
}

fn join_route(global_prefix: &str, controller_prefix: &str, method_path: &str) -> String {
    let mut parts = Vec::new();
    for seg in [global_prefix, controller_prefix, method_path] {
        let trimmed = seg.trim_matches('/');
        if !trimmed.is_empty() {
            parts.push(trimmed);
        }
    }
    format!("/{}", parts.join("/"))
}

fn property_key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        _ => None,
    }
}

fn summarize_ts_type(ty: &TSType<'_>) -> String {
    match ty {
        TSType::TSStringKeyword(_) => "string".into(),
        TSType::TSNumberKeyword(_) => "number".into(),
        TSType::TSBooleanKeyword(_) => "boolean".into(),
        TSType::TSNullKeyword(_) => "null".into(),
        TSType::TSUndefinedKeyword(_) => "undefined".into(),
        TSType::TSUnknownKeyword(_) => "unknown".into(),
        TSType::TSAnyKeyword(_) => "any".into(),
        TSType::TSVoidKeyword(_) => "void".into(),
        TSType::TSArrayType(arr) => format!("{}[]", summarize_ts_type(&arr.element_type)),
        TSType::TSUnionType(u) => u
            .types
            .iter()
            .map(summarize_ts_type)
            .collect::<Vec<_>>()
            .join(" | "),
        TSType::TSTypeReference(r) => match &r.type_name {
            oxc_ast::ast::TSTypeName::IdentifierReference(id) => id.name.to_string(),
            oxc_ast::ast::TSTypeName::QualifiedName(q) => q.right.name.to_string(),
            _ => "ref".into(),
        },
        _ => "object".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_nestjs_controller_without_body_leak() {
        let ts = r#"
@ApiTags('工作项')
@Controller('work-items')
export class WorkItemsController {
  @Patch(':workItemId')
  @RequireLocalPermissions(permissionCodes.workItemEdit)
  @ApiOperation({ summary: '按 expectedStateVersion 更新工作项内容' })
  async update(@Param('workItemId') workItemId: string, @Body() input: UpdateWorkItemDto) {
    const secretImplementationDetail = 'DO_NOT_LEAK_THIS_LINE';
    return this.service.update(workItemId, input, secretImplementationDetail);
  }
}
"#;
        let cards = extract_file_semantics("apps/api/src/work-items/work-items.controller.ts", ts);
        assert_eq!(cards.len(), 1);
        let c = &cards[0];
        assert_eq!(c.kind, SemanticKind::ApiEndpoint);
        assert!(c.title.contains("PATCH /api/v1/work-items/:workItemId"));
        assert_eq!(c.metadata.get("body_dto").map(String::as_str), Some("UpdateWorkItemDto"));
        assert!(!c.summary.contains("DO_NOT_LEAK_THIS_LINE"));
    }

    #[test]
    fn extracts_value_point_field_key_from_migration() {
        let ts = r#"
const fieldSpecs = [
  { key: 'qaValuePointRound1', label: '价值点（集测1轮）', fieldType: 'NUMBER', options: [] },
] as const;
"#;
        let cards = extract_file_semantics(
            "apps/api/src/database/migrations/1800000047000-yunxiao-requirement-fields.ts",
            ts,
        );
        assert!(cards.iter().any(|c| {
            c.metadata.get("field_key").map(String::as_str) == Some("qaValuePointRound1")
                && c.title.contains("价值点（集测1轮）")
        }));
    }
}
