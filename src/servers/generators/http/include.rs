//! `include` on a resource's `list` and `get_by_id` routes (wire contract
//! §7.5): which relationships a handler can include, and the helper that
//! gathers `included` for it.
//!
//! Included resources are read with their target module's `get_by_id`
//! through the `Fetch` helper, as a related link reads them (§9.3), so a
//! hand-written `get_by_id` and the scope rules apply to both alike.

use ontogen_core::ir::OpKind;

use super::relationship::Helper;
use super::{
    SCOPE, ScopeBinding, collection_expr, entity_type, helper_scope_param, is_scoped, linked_lookup, resource_names,
    served_resource,
};
use crate::resource::{Arity, Relationship};
use crate::servers::classify::classify_op;
use crate::servers::config::Config;
use crate::servers::parse::{ApiFn, ApiModule};

/// A field relationship a handler can include, with the module and
/// `get_by_id` its resources are fetched with.
struct Includable<'a> {
    rel: &'a Relationship,
    target: &'a ApiModule,
    get: &'a ApiFn,
}

impl Includable<'_> {
    /// Whether its resources are read under the prefix: from a scoped
    /// handler, when the target's `get_by_id` takes the store.
    fn scoped(&self, handler_scoped: bool) -> bool {
        handler_scoped && self.get.first_param_is_store
    }
}

/// The relationships of a resource module's type, as one of its `list` or
/// `get_by_id` handlers can include them.
pub(super) struct Includes<'a> {
    m: &'a ApiModule,
    config: &'a Config,
    type_name: &'a str,
    /// Whether the handler is the scoped one.
    scoped: bool,
    includable: Vec<Includable<'a>>,
    /// Every other relationship, in declaration order: a field one whose
    /// target module serves no `get_by_id`, which leaves no way to read its
    /// resources and no URL to link them at; a field one whose target's
    /// `get_by_id` takes a store, from an unscoped handler under a route
    /// prefix, which can neither open that store nor link the routes served
    /// only under the prefix; and every junction one, which has no linkage
    /// to include (§9.1).
    excluded: Vec<String>,
}

impl<'a> Includes<'a> {
    /// The relationships the `list` or `get_by_id` handler of resource
    /// module `m` in scope `scoped` can include.
    pub(super) fn new(m: &'a ApiModule, modules: &'a [ApiModule], config: &'a Config, scoped: bool) -> Self {
        let resource = config.resources.by_module(&m.name).expect("a resource handler serves a resource module");
        let mut includable = Vec::new();
        let mut excluded = Vec::new();
        for rel in &resource.relationships {
            match linked_lookup(modules, &rel.target_module) {
                Some((target, get)) if scoped || config.route_prefix.is_none() || !get.first_param_is_store => {
                    includable.push(Includable { rel, target, get });
                }
                _ => excluded.push(rel.name.clone()),
            }
        }
        let junctions = config
            .resources
            .junctions(m)
            .expect("`check_http_ops` refuses a module whose junction relationships do not build");
        excluded.extend(junctions.into_iter().map(|j| j.name));
        Includes { m, config, type_name: &resource.resource_type, scoped, includable, excluded }
    }

    /// The arguments of the handler's `QueryParams::include_paths` call.
    pub(super) fn spec_args(&self) -> String {
        let quoted = |names: &mut dyn Iterator<Item = &str>| names.map(|n| format!("\"{n}\"")).collect::<Vec<_>>();
        format!(
            "\"{}\", &[{}], &[{}]",
            self.type_name,
            quoted(&mut self.includable.iter().map(|i| i.rel.name.as_str())).join(", "),
            quoted(&mut self.excluded.iter().map(String::as_str)).join(", "),
        )
    }

    fn helper_name(&self) -> String {
        let suffix = if self.scoped { "_scoped" } else { "" };
        format!("ontogen_{}_included{suffix}", self.m.name)
    }

    /// Whether the helper reads any resource under the prefix, and so takes
    /// the scope.
    fn takes_scope(&self) -> bool {
        self.includable.iter().any(|i| i.scoped(self.scoped))
    }

    /// The handler lines that give `document` its `included` when the
    /// request carried `include`, for the primary data's `entities` (a
    /// `&[Entity]` expression).
    pub(super) fn handler_lines(&self, entities: &str) -> String {
        if self.includable.is_empty() {
            return "    if include.is_some() {\n        document = document.with_included(Vec::new());\n    }\n"
                .to_string();
        }
        let scope = if self.takes_scope() { format!("&{SCOPE}, ") } else { String::new() };
        format!(
            "    if let Some(paths) = &include {{\n        document = \
             document.with_included({}(&ontogen_state, {scope}{entities}, paths).await?);\n    }}\n",
            self.helper_name()
        )
    }

    /// Emit the helper [`handler_lines`](Self::handler_lines) calls.
    fn emit_helper(&self, out: &mut String) {
        let config = self.config;
        let resource = config.resources.by_module(&self.m.name).expect("a resource module");
        let entity_ty = entity_type(self.m).expect("a resource module names its entity");
        let scope_param =
            config.route_prefix.as_ref().filter(|_| self.takes_scope()).map(helper_scope_param).unwrap_or_default();
        // Each path's block: its ids not yet seen, fetched and built as the
        // target's own resource objects. The file is formatted once
        // written, so the blocks need no indentation.
        let mut blocks: Vec<(&str, String)> = Vec::new();
        for inc in &self.includable {
            let rel = inc.rel;
            let field = &rel.field;
            let ids = match rel.arity {
                Arity::ToOne { nullable: true } => {
                    format!("entities.iter().filter_map(|entity| entity.{field}.as_deref())")
                }
                Arity::ToOne { nullable: false } => format!("entities.iter().map(|entity| entity.{field}.as_str())"),
                Arity::ToMany => {
                    format!("entities.iter().flat_map(|entity| entity.{field}.iter().map(String::as_str))")
                }
            };
            let scoped = inc.scoped(self.scoped);
            let prefix = config.route_prefix.as_ref().filter(|_| scoped);
            let collection = collection_expr(&rel.target_type, prefix.map(|p| (p, ScopeBinding::Borrowed)));
            let scope_arg = if scoped { format!("{SCOPE}, ") } else { String::new() };
            blocks.push((
                &rel.name,
                format!(
                    "{{\nlet ids = included.new_ids(\"{}\", {ids});\nlet collection = {collection};\nfor related in \
                     {}(state, {scope_arg}&ids).await? {{\nincluded.push({}(&related, collection))?;\n}}\n}}",
                    rel.target_type,
                    Helper::Fetch.name(&inc.target.name, scoped),
                    resource_names(&inc.target.name).resource,
                ),
            ));
        }
        // A lone path is compared, as clippy's `single_match` asks.
        let each_path = match blocks.as_slice() {
            [(name, block)] => format!("if *path == \"{name}\" {block}"),
            _ => {
                let arms: String = blocks.iter().map(|(name, block)| format!("\"{name}\" => {block}\n")).collect();
                format!("match *path {{\n{arms}_ => {{}}\n}}")
            }
        };
        out.push_str(&format!(
            "/// The resources `paths` include for `entities`, path by path and each in\n/// linkage order: each \
             once, and none of `entities` among them. `paths` are\n/// those `include_paths` admitted, so no other \
             path occurs.\nasync fn {}(\n    state: &{},\n    {scope_param}entities: &[{entity_ty}],\n    \
             paths: &[&str],\n) -> Result<Vec<OntogenAnyResource>, OntogenErrorObject> {{\n    let mut included = \
             OntogenIncluded::new(\"{}\", entities.iter().map(|entity| entity.{}.as_str()));\n    for path in paths \
             {{\n{each_path}\n}}\n    Ok(included.finish())\n}}\n\n",
            self.helper_name(),
            config.state_type,
            self.type_name,
            resource.id_field,
        ));
    }
}

/// The resource modules of `modules` whose `list` or `get_by_id` is served,
/// each with the scope of those handlers, once per module and scope.
fn handlers<'a>(modules: &'a [ApiModule], config: &Config) -> Vec<(&'a ApiModule, bool)> {
    let mut found: Vec<(&ApiModule, bool)> = Vec::new();
    for m in modules {
        for f in &m.functions {
            let reads_include = matches!(classify_op(m, f), OpKind::List | OpKind::GetById);
            if !reads_include || served_resource(m, f, config).is_none() {
                continue;
            }
            let scoped = is_scoped(f, config);
            if !found.iter().any(|(fm, s)| fm.name == m.name && *s == scoped) {
                found.push((m, scoped));
            }
        }
    }
    found
}

/// The `Fetch` helpers the `include` helpers call, as the target module,
/// its `get_by_id` and whether it is read under the prefix.
pub(super) fn fetches<'a>(modules: &'a [ApiModule], config: &'a Config) -> Vec<(&'a ApiModule, &'a ApiFn, bool)> {
    handlers(modules, config)
        .into_iter()
        .flat_map(|(m, scoped)| {
            Includes::new(m, modules, config, scoped)
                .includable
                .into_iter()
                .map(move |inc| (inc.target, inc.get, inc.scoped(scoped)))
        })
        .collect()
}

/// Emit the `include` helper of every `list` and `get_by_id` handler that
/// can include a relationship, once per module and scope.
pub(super) fn emit_helpers(out: &mut String, modules: &[ApiModule], config: &Config) {
    for (m, scoped) in handlers(modules, config) {
        let includes = Includes::new(m, modules, config, scoped);
        if !includes.includable.is_empty() {
            includes.emit_helper(out);
        }
    }
}
