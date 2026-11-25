use crate::schema::{ArchSpec, EdgeKind, SPEC_VERSION};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationIssue {
    pub severity: IssueSeverity,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub valid: bool,
    pub error_count: usize,
    pub warning_count: usize,
    pub issues: Vec<ValidationIssue>,
}

pub fn validate_spec(spec: &ArchSpec) -> ValidationReport {
    let mut issues = Vec::new();

    if spec.schema_version != SPEC_VERSION {
        issues.push(ValidationIssue {
            severity: IssueSeverity::Warning,
            path: "schema_version".to_string(),
            message: format!(
                "spec schema_version={} differs from expected {}",
                spec.schema_version, SPEC_VERSION
            ),
        });
    }

    if spec.title.trim().is_empty() {
        issues.push(ValidationIssue {
            severity: IssueSeverity::Error,
            path: "title".to_string(),
            message: "document title must not be empty".to_string(),
        });
    }

    if spec.diagrams.is_empty() && spec.flow_table.is_none() {
        issues.push(ValidationIssue {
            severity: IssueSeverity::Error,
            path: "diagrams".to_string(),
            message: "spec must define at least one diagram or a flow_table".to_string(),
        });
    }

    let mut diagram_ids = HashSet::new();
    for (di, diag) in spec.diagrams.iter().enumerate() {
        let dpath = format!("diagrams[{di}]");
        if diag.id.trim().is_empty() {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                path: format!("{dpath}.id"),
                message: "diagram id must not be empty".to_string(),
            });
        } else if !diagram_ids.insert(diag.id.clone()) {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                path: format!("{dpath}.id"),
                message: format!("duplicate diagram id '{}'", diag.id),
            });
        }

        if diag.heading.trim().is_empty() {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                path: format!("{dpath}.heading"),
                message: "diagram heading must not be empty".to_string(),
            });
        }

        if diag.columns.is_empty() {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                path: format!("{dpath}.columns"),
                message: "diagram must contain at least one column".to_string(),
            });
        }

        let mut col_ids = HashSet::new();
        let mut card_ids = HashSet::new();

        for (ci, col) in diag.columns.iter().enumerate() {
            let cpath = format!("{dpath}.columns[{ci}]");
            if col.id.trim().is_empty() {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    path: format!("{cpath}.id"),
                    message: "column id must not be empty".to_string(),
                });
            } else if !col_ids.insert(col.id.clone()) {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    path: format!("{cpath}.id"),
                    message: format!("duplicate column id '{}'", col.id),
                });
            }

            if let Some(w) = col.width
                && !(120.0..=1600.0).contains(&w)
            {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    path: format!("{cpath}.width"),
                    message: format!("column width {w} out of valid range [120, 1600]"),
                });
            }

            for (ki, card) in col.cards.iter().enumerate() {
                let kpath = format!("{cpath}.cards[{ki}]");
                if card.id.trim().is_empty() {
                    issues.push(ValidationIssue {
                        severity: IssueSeverity::Error,
                        path: format!("{kpath}.id"),
                        message: "card id must not be empty".to_string(),
                    });
                } else if !card_ids.insert(card.id.clone()) {
                    issues.push(ValidationIssue {
                        severity: IssueSeverity::Error,
                        path: format!("{kpath}.id"),
                        message: format!("duplicate card id '{}' in diagram '{}'", card.id, diag.id),
                    });
                }
                if card.title.trim().is_empty() {
                    issues.push(ValidationIssue {
                        severity: IssueSeverity::Error,
                        path: format!("{kpath}.title"),
                        message: "card title must not be empty".to_string(),
                    });
                }
            }
        }

        // Validate align_with references.
        for (ci, col) in diag.columns.iter().enumerate() {
            for (ki, card) in col.cards.iter().enumerate() {
                if let Some(target) = &card.align_with
                    && !card_ids.contains(target)
                {
                    issues.push(ValidationIssue {
                        severity: IssueSeverity::Error,
                        path: format!("{dpath}.columns[{ci}].cards[{ki}].align_with"),
                        message: format!("align_with references unknown card id '{target}'"),
                    });
                }
            }
        }

        // Validate edges.
        for (ei, edge) in diag.edges.iter().enumerate() {
            let epath = format!("{dpath}.edges[{ei}]");
            if edge.from.is_empty() {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    path: format!("{epath}.from"),
                    message: "edge must have at least one source card id".to_string(),
                });
            }
            if edge.to.is_empty() {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    path: format!("{epath}.to"),
                    message: "edge must have at least one target card id".to_string(),
                });
            }
            if matches!(
                edge.kind,
                EdgeKind::Direct | EdgeKind::LoopLeft | EdgeKind::BottomRoute
            ) && (edge.from.len() != 1 || edge.to.len() != 1)
            {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Warning,
                    path: epath.clone(),
                    message: "non-bus edge typically has 1 source and 1 target; extra endpoints will be ignored".to_string(),
                });
            }
            for src in &edge.from {
                if !card_ids.contains(src) {
                    issues.push(ValidationIssue {
                        severity: IssueSeverity::Error,
                        path: format!("{epath}.from"),
                        message: format!("edge references unknown source card id '{src}'"),
                    });
                }
            }
            for dst in &edge.to {
                if !card_ids.contains(dst) {
                    issues.push(ValidationIssue {
                        severity: IssueSeverity::Error,
                        path: format!("{epath}.to"),
                        message: format!("edge references unknown target card id '{dst}'"),
                    });
                }
            }
        }
    }

    if let Some(table) = &spec.flow_table {
        if table.heading.trim().is_empty() {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                path: "flow_table.heading".to_string(),
                message: "flow_table heading must not be empty".to_string(),
            });
        }
        for (gi, grp) in table.groups.iter().enumerate() {
            if grp.name.trim().is_empty() {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    path: format!("flow_table.groups[{gi}].name"),
                    message: "flow group name must not be empty".to_string(),
                });
            }
            for (ri, row) in grp.rows.iter().enumerate() {
                if row.action.trim().is_empty() {
                    issues.push(ValidationIssue {
                        severity: IssueSeverity::Error,
                        path: format!("flow_table.groups[{gi}].rows[{ri}].action"),
                        message: "flow row action must not be empty".to_string(),
                    });
                }
            }
        }
    }

    let error_count = issues
        .iter()
        .filter(|i| i.severity == IssueSeverity::Error)
        .count();
    let warning_count = issues
        .iter()
        .filter(|i| i.severity == IssueSeverity::Warning)
        .count();

    ValidationReport {
        valid: error_count == 0,
        error_count,
        warning_count,
        issues,
    }
}
