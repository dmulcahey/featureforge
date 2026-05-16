#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use syn::parse::Parser;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

fn strip_use_alias(path: &str) -> &str {
    path.split_once(" as ")
        .map_or(path, |(before_alias, _)| before_alias)
        .trim()
}

fn combine_use_path(prefix: &str, suffix: &str) -> String {
    let suffix = strip_use_alias(suffix).trim_end_matches("::");
    if suffix.is_empty() || suffix == "self" {
        prefix.to_owned()
    } else if prefix.is_empty() {
        suffix.to_owned()
    } else {
        format!("{prefix}::{suffix}")
    }
}

fn collect_use_paths_from_tree(prefix: &str, tree: &syn::UseTree, paths: &mut Vec<String>) {
    match tree {
        syn::UseTree::Path(path) => {
            let prefix = combine_use_path(prefix, &path.ident.to_string());
            collect_use_paths_from_tree(&prefix, &path.tree, paths);
        }
        syn::UseTree::Rename(rename) => {
            paths.push(combine_use_path(prefix, &rename.ident.to_string()));
        }
        syn::UseTree::Name(name) => {
            paths.push(combine_use_path(prefix, &name.ident.to_string()));
        }
        syn::UseTree::Glob(_) => {
            paths.push(combine_use_path(prefix, "*"));
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_use_paths_from_tree(prefix, item, paths);
            }
        }
    }
}

struct UsePathCollector {
    paths: Vec<String>,
}

impl<'ast> Visit<'ast> for UsePathCollector {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if attrs_include_test_only_cfg(&item.attrs) {
            return;
        }
        visit::visit_item_mod(self, item);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        if attrs_include_test_only_cfg(&item.attrs) {
            return;
        }
        collect_use_paths_from_tree("", &item.tree, &mut self.paths);
        visit::visit_item_use(self, item);
    }
}

pub fn expanded_use_paths(source: &str) -> Vec<String> {
    let syntax = syn::parse_file(source)
        .unwrap_or_else(|error| panic!("source should parse for use expansion: {error}"));
    let mut collector = UsePathCollector { paths: Vec::new() };
    collector.visit_file(&syntax);
    collector.paths.sort();
    collector.paths.dedup();
    collector.paths
}

pub fn production_source_uses_parent_glob(source: &str) -> bool {
    expanded_use_paths(source)
        .into_iter()
        .any(|path| path == "super::*" || (path.starts_with("super::") && path.ends_with("::*")))
}

pub fn source_tree_declares_test(root: &Path, test_name: &str) -> bool {
    source_tree_declares_test_in_dir(&root.join("src"), test_name)
        || source_tree_declares_test_in_dir(&root.join("tests"), test_name)
}

fn source_tree_declares_test_in_dir(dir: &Path, test_name: &str) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            if source_tree_declares_test_in_dir(&path, test_name) {
                return true;
            }
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()));
        if source_declares_test_function(&source, test_name) {
            return true;
        }
    }
    false
}

pub fn source_declares_test_function(content: &str, test_name: &str) -> bool {
    let declaration = format!("fn {test_name}(");
    let mut saw_test_attr = false;
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line == "#[test]"
            || line.starts_with("#[tokio::test")
            || line.starts_with("#[rstest")
            || line.starts_with("#[case]")
        {
            saw_test_attr = true;
            continue;
        }
        if saw_test_attr && line.starts_with(&declaration) {
            return true;
        }
        if !line.is_empty() && !line.starts_with("#[") {
            saw_test_attr = false;
        }
    }
    false
}

pub fn normalized_expanded_use_paths(rel: &str, source: &str) -> Vec<String> {
    expanded_use_paths(source)
        .into_iter()
        .map(|path| normalize_use_path_for_source(rel, &path))
        .collect()
}

fn module_path_for_source(rel: &str) -> String {
    let path = rel
        .strip_prefix("src/")
        .unwrap_or_else(|| panic!("{rel} should be a src-relative Rust module path"))
        .strip_suffix(".rs")
        .unwrap_or_else(|| panic!("{rel} should be a Rust source file"));
    let mut segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if segments.last().is_some_and(|segment| segment == "mod") {
        segments.pop();
    }
    assert!(
        !segments.is_empty(),
        "{rel} should map to a non-empty Rust module path"
    );

    let mut module_segments = Vec::with_capacity(segments.len() + 1);
    module_segments.push(String::from("crate"));
    module_segments.extend(segments);
    module_segments.join("::")
}

pub fn normalize_use_path_for_source(rel: &str, path: &str) -> String {
    let segments = path
        .split("::")
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let Some(first_segment) = segments.first().copied() else {
        return String::new();
    };

    if first_segment == "crate" {
        return segments.join("::");
    }

    if matches!(first_segment, "execution" | "workflow") {
        let mut absolute_segments = Vec::with_capacity(segments.len() + 1);
        absolute_segments.push(String::from("crate"));
        absolute_segments.extend(segments.iter().map(|segment| (*segment).to_owned()));
        return absolute_segments.join("::");
    }

    if matches!(first_segment, "self" | "super") && !rel.starts_with("src/") {
        return segments.join("::");
    }

    if !matches!(first_segment, "self" | "super") {
        return segments.join("::");
    }

    let module_path = module_path_for_source(rel);
    let mut absolute_segments = module_path
        .split("::")
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let remaining_segments = if first_segment == "self" {
        &segments[1..]
    } else {
        for segment in &segments {
            if *segment != "super" {
                break;
            }
            if absolute_segments.len() > 1 {
                absolute_segments.pop();
            }
        }
        let consumed_super_segments = segments
            .iter()
            .take_while(|segment| **segment == "super")
            .count();
        &segments[consumed_super_segments..]
    };
    absolute_segments.extend(
        remaining_segments
            .iter()
            .map(|segment| (*segment).to_owned()),
    );
    absolute_segments.join("::")
}

pub fn parse_rust_source(rel: &str, source: &str) -> syn::File {
    syn::parse_file(source).unwrap_or_else(|error| {
        panic!("{rel} should parse as Rust source for boundary checks: {error}")
    })
}

pub fn syn_path_to_string(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

fn collect_use_aliases_from_tree(
    rel: &str,
    prefix: &str,
    tree: &syn::UseTree,
    aliases: &mut BTreeMap<String, String>,
) {
    match tree {
        syn::UseTree::Path(path) => {
            let prefix = combine_use_path(prefix, &path.ident.to_string());
            collect_use_aliases_from_tree(rel, &prefix, &path.tree, aliases);
        }
        syn::UseTree::Rename(rename) => {
            let target = combine_use_path(prefix, &rename.ident.to_string());
            aliases.insert(
                rename.rename.to_string(),
                normalize_use_path_for_source(rel, &target),
            );
        }
        syn::UseTree::Name(name) => {
            let target = combine_use_path(prefix, &name.ident.to_string());
            aliases.insert(
                name.ident.to_string(),
                normalize_use_path_for_source(rel, &target),
            );
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_use_aliases_from_tree(rel, prefix, item, aliases);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}

struct UseAliasCollector<'a> {
    rel: &'a str,
    include_test_cfg_items: bool,
    aliases: BTreeMap<String, String>,
}

impl<'ast> Visit<'ast> for UseAliasCollector<'_> {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if should_skip_test_only_cfg(self.include_test_cfg_items, &item.attrs) {
            return;
        }
        visit::visit_item_mod(self, item);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        if should_skip_test_only_cfg(self.include_test_cfg_items, &item.attrs) {
            return;
        }
        collect_use_aliases_from_tree(self.rel, "", &item.tree, &mut self.aliases);
        visit::visit_item_use(self, item);
    }
}

pub fn use_aliases(rel: &str, syntax: &syn::File) -> BTreeMap<String, String> {
    let mut collector = UseAliasCollector {
        rel,
        include_test_cfg_items: include_test_cfg_items_for_source(rel),
        aliases: BTreeMap::new(),
    };
    collector.visit_file(syntax);
    collector.aliases
}

pub fn normalize_code_path_for_source(
    rel: &str,
    path: &str,
    aliases: &BTreeMap<String, String>,
) -> String {
    let mut segments = path.split("::").filter(|segment| !segment.is_empty());
    let Some(first) = segments.next() else {
        return String::new();
    };
    if let Some(alias_target) = aliases.get(first) {
        let mut normalized = alias_target.clone();
        for segment in segments {
            normalized.push_str("::");
            normalized.push_str(segment);
        }
        normalized
    } else {
        normalize_use_path_for_source(rel, path)
    }
}

pub struct AdditionalGlobAliasSource<'a> {
    pub glob_path: &'a str,
    pub source_rel: &'a str,
    pub source: &'a str,
}

pub fn aliases_for_source(
    rel: &str,
    source: &str,
    syntax: &syn::File,
    additional_glob_aliases: &[AdditionalGlobAliasSource<'_>],
) -> BTreeMap<String, String> {
    let mut aliases = BTreeMap::new();
    let source_imports = normalized_expanded_use_paths(rel, source);
    for additional in additional_glob_aliases {
        if source_imports
            .iter()
            .any(|path| path == additional.glob_path)
        {
            let alias_syntax = parse_rust_source(additional.source_rel, additional.source);
            aliases.extend(use_aliases(additional.source_rel, &alias_syntax));
        }
    }
    aliases.extend(use_aliases(rel, syntax));
    aliases
}

pub fn macro_token_path_candidates(tokens: proc_macro2::TokenStream) -> Vec<String> {
    let mut paths = Vec::new();
    collect_macro_token_path_candidates(tokens, &mut paths);
    paths.sort();
    paths.dedup();
    paths
}

fn collect_macro_token_path_candidates(tokens: proc_macro2::TokenStream, paths: &mut Vec<String>) {
    let mut current_segments = Vec::<String>::new();
    let mut awaiting_path_segment = false;

    for token in tokens {
        match token {
            proc_macro2::TokenTree::Group(group) => {
                flush_macro_token_path(&mut current_segments, paths);
                awaiting_path_segment = false;
                collect_macro_token_path_candidates(group.stream(), paths);
            }
            proc_macro2::TokenTree::Ident(ident) => {
                if !current_segments.is_empty() && !awaiting_path_segment {
                    flush_macro_token_path(&mut current_segments, paths);
                }
                current_segments.push(ident.to_string());
                awaiting_path_segment = false;
            }
            proc_macro2::TokenTree::Punct(punct) if punct.as_char() == ':' => {
                if !current_segments.is_empty() {
                    awaiting_path_segment = true;
                }
            }
            proc_macro2::TokenTree::Punct(_) | proc_macro2::TokenTree::Literal(_) => {
                flush_macro_token_path(&mut current_segments, paths);
                awaiting_path_segment = false;
            }
        }
    }

    flush_macro_token_path(&mut current_segments, paths);
}

fn flush_macro_token_path(current_segments: &mut Vec<String>, paths: &mut Vec<String>) {
    if current_segments.is_empty() {
        return;
    }
    paths.push(current_segments.join("::"));
    current_segments.clear();
}

fn cfg_predicate_is_test_only(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::List(list) if list.path.is_ident("not") => false,
        syn::Meta::List(list) if list.path.is_ident("all") => list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|items| items.iter().any(cfg_predicate_is_test_only)),
        syn::Meta::List(list) if list.path.is_ident("any") => list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|items| !items.is_empty() && items.iter().all(cfg_predicate_is_test_only)),
        _ => false,
    }
}

fn cfg_attr_is_test_only(attr: &syn::Attribute) -> bool {
    attr.path().is_ident("cfg")
        && matches!(&attr.meta, syn::Meta::List(list) if list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|items| items.iter().any(cfg_predicate_is_test_only)))
}

pub fn attrs_include_test_only_cfg(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(cfg_attr_is_test_only)
}

pub fn phase_detail_literals_from_source(rel: &str, source: &str) -> Vec<String> {
    let syntax = parse_rust_source(rel, source);
    let aliases = use_aliases(rel, &syntax);
    let mut literals = syntax
        .items
        .iter()
        .filter_map(|item| {
            let syn::Item::Const(item_const) = item else {
                return None;
            };
            item_const
                .ident
                .to_string()
                .starts_with("DETAIL_")
                .then(|| phase_detail_const_expr_value(rel, &aliases, &item_const.expr))
                .flatten()
        })
        .collect::<Vec<_>>();
    literals.sort();
    literals.dedup();
    literals
}

fn phase_detail_const_expr_value(
    rel: &str,
    aliases: &BTreeMap<String, String>,
    expr: &syn::Expr,
) -> Option<String> {
    match expr {
        syn::Expr::Lit(literal) => {
            if let syn::Lit::Str(literal) = &literal.lit {
                Some(literal.value())
            } else {
                None
            }
        }
        syn::Expr::Macro(macro_expr) => {
            let raw_macro_path = syn_path_to_string(&macro_expr.mac.path);
            let normalized_macro_path =
                normalize_code_path_for_source(rel, &raw_macro_path, aliases);
            if normalized_macro_path.rsplit("::").next() != Some("concat") {
                return None;
            }
            macro_expr
                .mac
                .parse_body_with(
                    syn::punctuated::Punctuated::<syn::LitStr, syn::Token![,]>::parse_terminated,
                )
                .ok()
                .map(|parts| parts.iter().map(syn::LitStr::value).collect())
        }
        syn::Expr::Paren(paren) => phase_detail_const_expr_value(rel, aliases, &paren.expr),
        syn::Expr::Group(group) => phase_detail_const_expr_value(rel, aliases, &group.expr),
        _ => None,
    }
}

struct StringLiteralValueCollector<'a> {
    rel: &'a str,
    aliases: &'a BTreeMap<String, String>,
    values: Vec<String>,
}

impl<'ast> Visit<'ast> for StringLiteralValueCollector<'_> {
    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.values.push(literal.value());
    }

    fn visit_expr_method_call(&mut self, method_call: &'ast syn::ExprMethodCall) {
        if let Some(value) = literal_array_method_value(self.rel, self.aliases, method_call) {
            self.values.push(value);
        }
        visit::visit_expr_method_call(self, method_call);
    }

    fn visit_macro(&mut self, macro_call: &'ast syn::Macro) {
        if let Some(value) = macro_literal_value(self.rel, self.aliases, macro_call) {
            self.values.push(value);
        }
        collect_string_literal_values_from_tokens(
            self.rel,
            self.aliases,
            macro_call.tokens.clone(),
            &mut self.values,
        );
        visit::visit_macro(self, macro_call);
    }
}

fn collect_string_literal_values_from_tokens(
    rel: &str,
    aliases: &BTreeMap<String, String>,
    tokens: proc_macro2::TokenStream,
    values: &mut Vec<String>,
) {
    let tokens = tokens.into_iter().collect::<Vec<_>>();
    let mut index = 0;
    while index < tokens.len() {
        if let (
            proc_macro2::TokenTree::Ident(ident),
            Some(proc_macro2::TokenTree::Punct(punct)),
            Some(proc_macro2::TokenTree::Group(group)),
        ) = (&tokens[index], tokens.get(index + 1), tokens.get(index + 2))
            && matches!(
                ident.to_string().as_str(),
                "concat" | "format" | "format_args"
            )
            && punct.as_char() == '!'
        {
            match ident.to_string().as_str() {
                "concat" => {
                    if let Some(value) =
                        concat_literal_value_from_tokens(rel, aliases, group.stream())
                    {
                        values.push(value);
                    }
                }
                "format" | "format_args" => {
                    if let Some(value) =
                        format_literal_value_from_tokens(rel, aliases, group.stream())
                    {
                        values.push(value);
                    }
                }
                _ => {}
            }
            collect_string_literal_values_from_tokens(rel, aliases, group.stream(), values);
            index += 3;
            continue;
        }

        match &tokens[index] {
            proc_macro2::TokenTree::Group(group) => {
                collect_string_literal_values_from_tokens(rel, aliases, group.stream(), values);
            }
            proc_macro2::TokenTree::Literal(literal) => {
                if let Ok(literal) = syn::parse_str::<syn::LitStr>(&literal.to_string()) {
                    values.push(literal.value());
                }
            }
            proc_macro2::TokenTree::Ident(_) | proc_macro2::TokenTree::Punct(_) => {}
        }
        index += 1;
    }
}

fn macro_literal_value(
    rel: &str,
    aliases: &BTreeMap<String, String>,
    macro_call: &syn::Macro,
) -> Option<String> {
    let raw_macro_path = syn_path_to_string(&macro_call.path);
    let normalized_macro_path = normalize_code_path_for_source(rel, &raw_macro_path, aliases);
    match normalized_macro_path.rsplit("::").next()? {
        "concat" => concat_literal_value_from_tokens(rel, aliases, macro_call.tokens.clone()),
        "format" | "format_args" => {
            format_literal_value_from_tokens(rel, aliases, macro_call.tokens.clone())
        }
        _ => None,
    }
}

fn concat_literal_value_from_tokens(
    rel: &str,
    aliases: &BTreeMap<String, String>,
    tokens: proc_macro2::TokenStream,
) -> Option<String> {
    let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
    parser
        .parse2(tokens)
        .ok()
        .and_then(|parts| {
            parts
                .iter()
                .map(|expr| string_expr_value(rel, aliases, expr))
                .collect::<Option<Vec<_>>>()
        })
        .filter(|parts| !parts.is_empty())
        .map(|parts| parts.concat())
}

fn format_literal_value_from_tokens(
    rel: &str,
    aliases: &BTreeMap<String, String>,
    tokens: proc_macro2::TokenStream,
) -> Option<String> {
    let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
    let mut parts = parser.parse2(tokens).ok()?.into_iter();
    let template = string_expr_value(rel, aliases, &parts.next()?)?;
    let args = parts
        .map(|expr| string_expr_value(rel, aliases, &expr))
        .collect::<Option<Vec<_>>>()?;
    apply_simple_format_template(&template, &args)
}

fn apply_simple_format_template(template: &str, args: &[String]) -> Option<String> {
    let mut output = String::new();
    let mut chars = template.chars().peekable();
    let mut arg_index = 0;
    while let Some(ch) = chars.next() {
        match ch {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                output.push('{');
            }
            '{' if chars.peek() == Some(&'}') => {
                chars.next();
                let arg = args.get(arg_index)?;
                output.push_str(arg);
                arg_index += 1;
            }
            '{' => return None,
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                output.push('}');
            }
            '}' => return None,
            _ => output.push(ch),
        }
    }
    (arg_index == args.len()).then_some(output)
}

fn literal_array_method_value(
    rel: &str,
    aliases: &BTreeMap<String, String>,
    method_call: &syn::ExprMethodCall,
) -> Option<String> {
    let parts = string_array_values_from_expr(rel, aliases, &method_call.receiver)?;
    match method_call.method.to_string().as_str() {
        "concat" if method_call.args.is_empty() => Some(parts.concat()),
        "join" if method_call.args.len() == 1 => {
            let separator = string_expr_value(rel, aliases, method_call.args.first()?)?;
            Some(parts.join(&separator))
        }
        _ => None,
    }
}

fn string_array_values_from_expr(
    rel: &str,
    aliases: &BTreeMap<String, String>,
    expr: &syn::Expr,
) -> Option<Vec<String>> {
    match expr {
        syn::Expr::Array(array) => array
            .elems
            .iter()
            .map(|expr| string_expr_value(rel, aliases, expr))
            .collect(),
        syn::Expr::Group(group) => string_array_values_from_expr(rel, aliases, &group.expr),
        syn::Expr::Paren(paren) => string_array_values_from_expr(rel, aliases, &paren.expr),
        syn::Expr::Reference(reference) => {
            string_array_values_from_expr(rel, aliases, &reference.expr)
        }
        _ => None,
    }
}

fn string_expr_value(
    rel: &str,
    aliases: &BTreeMap<String, String>,
    expr: &syn::Expr,
) -> Option<String> {
    match expr {
        syn::Expr::Lit(literal) => {
            if let syn::Lit::Str(literal) = &literal.lit {
                Some(literal.value())
            } else {
                None
            }
        }
        syn::Expr::Macro(macro_expr) => macro_literal_value(rel, aliases, &macro_expr.mac),
        syn::Expr::MethodCall(method_call) => literal_array_method_value(rel, aliases, method_call),
        syn::Expr::Group(group) => string_expr_value(rel, aliases, &group.expr),
        syn::Expr::Paren(paren) => string_expr_value(rel, aliases, &paren.expr),
        syn::Expr::Reference(reference) => string_expr_value(rel, aliases, &reference.expr),
        _ => None,
    }
}

pub fn rust_string_literal_values(rel: &str, source: &str) -> Vec<String> {
    let syntax = parse_rust_source(rel, source);
    let aliases = use_aliases(rel, &syntax);
    let mut collector = StringLiteralValueCollector {
        rel,
        aliases: &aliases,
        values: Vec::new(),
    };
    collector.visit_file(&syntax);
    collector.values
}

struct ProductionStringLiteralValueCollector<'a> {
    rel: &'a str,
    aliases: &'a BTreeMap<String, String>,
    values: Vec<String>,
}

impl<'ast> Visit<'ast> for ProductionStringLiteralValueCollector<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if attrs_are_test_only(node.attrs.as_slice()) {
            return;
        }
        visit::visit_item_mod(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if attrs_are_test_only(node.attrs.as_slice()) {
            return;
        }
        visit::visit_item_fn(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if attrs_are_test_only(node.attrs.as_slice()) {
            return;
        }
        visit::visit_item_impl(self, node);
    }

    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        if attrs_are_test_only(node.attrs.as_slice()) {
            return;
        }
        visit::visit_item_const(self, node);
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        if attrs_are_test_only(node.attrs.as_slice()) {
            return;
        }
        visit::visit_item_static(self, node);
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.values.push(literal.value());
    }

    fn visit_expr_method_call(&mut self, method_call: &'ast syn::ExprMethodCall) {
        if let Some(value) = literal_array_method_value(self.rel, self.aliases, method_call) {
            self.values.push(value);
        }
        visit::visit_expr_method_call(self, method_call);
    }

    fn visit_macro(&mut self, macro_call: &'ast syn::Macro) {
        if let Some(value) = macro_literal_value(self.rel, self.aliases, macro_call) {
            self.values.push(value);
        }
        collect_string_literal_values_from_tokens(
            self.rel,
            self.aliases,
            macro_call.tokens.clone(),
            &mut self.values,
        );
        visit::visit_macro(self, macro_call);
    }
}

fn attrs_are_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs_include_test_only_cfg(attrs) || attrs.iter().any(|attr| attr.path().is_ident("test"))
}

pub fn rust_production_string_literal_values(rel: &str, source: &str) -> Vec<String> {
    let syntax = parse_rust_source(rel, source);
    let aliases = use_aliases(rel, &syntax);
    let mut collector = ProductionStringLiteralValueCollector {
        rel,
        aliases: &aliases,
        values: Vec::new(),
    };
    collector.visit_file(&syntax);
    collector.values
}

pub fn phase_detail_literal_value_violations(
    rel: &str,
    source: &str,
    known_phase_details: &[String],
    allowed_context: &str,
) -> Vec<String> {
    let known_phase_details = known_phase_details
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut violations = rust_string_literal_values(rel, source)
        .into_iter()
        .filter(|literal| known_phase_details.contains(literal.as_str()))
        .map(|literal| {
            format!("{rel} duplicates phase-detail literal `{literal}` {allowed_context}")
        })
        .collect::<Vec<_>>();
    violations.sort();
    violations.dedup();
    violations
}

fn is_phase_detail_shaped_literal(literal: &str) -> bool {
    let Some(suffix) = literal.rsplit('_').next() else {
        return false;
    };
    literal.contains('_')
        && literal
            .chars()
            .all(|character| character.is_ascii_lowercase() || character == '_')
        && matches!(
            suffix,
            "bug" | "gate" | "pending" | "progress" | "ready" | "reconcile" | "required"
        )
}

fn collect_string_values_from_expr(
    rel: &str,
    aliases: &BTreeMap<String, String>,
    expr: &syn::Expr,
    values: &mut Vec<String>,
) {
    let mut collector = StringLiteralValueCollector {
        rel,
        aliases,
        values: Vec::new(),
    };
    collector.visit_expr(expr);
    values.extend(collector.values);
}

fn collect_string_values_from_pat(pat: &syn::Pat, values: &mut Vec<String>) {
    match pat {
        syn::Pat::Lit(lit) => {
            if let syn::Lit::Str(literal) = &lit.lit {
                values.push(literal.value());
            }
        }
        syn::Pat::Or(or_pat) => {
            for case in &or_pat.cases {
                collect_string_values_from_pat(case, values);
            }
        }
        syn::Pat::Reference(reference) => collect_string_values_from_pat(&reference.pat, values),
        syn::Pat::Tuple(tuple) => {
            for elem in &tuple.elems {
                collect_string_values_from_pat(elem, values);
            }
        }
        syn::Pat::TupleStruct(tuple) => {
            for elem in &tuple.elems {
                collect_string_values_from_pat(elem, values);
            }
        }
        syn::Pat::Struct(struct_pat) => {
            for field in &struct_pat.fields {
                collect_string_values_from_pat(&field.pat, values);
            }
        }
        syn::Pat::Slice(slice) => {
            for elem in &slice.elems {
                collect_string_values_from_pat(elem, values);
            }
        }
        syn::Pat::Paren(paren) => collect_string_values_from_pat(&paren.pat, values),
        syn::Pat::Type(typed) => collect_string_values_from_pat(&typed.pat, values),
        syn::Pat::Const(_)
        | syn::Pat::Ident(_)
        | syn::Pat::Macro(_)
        | syn::Pat::Path(_)
        | syn::Pat::Range(_)
        | syn::Pat::Rest(_)
        | syn::Pat::Verbatim(_)
        | syn::Pat::Wild(_) => {}
        _ => {}
    }
}

fn path_mentions_phase_detail(path: &syn::Path) -> bool {
    path.segments
        .iter()
        .any(|segment| segment.ident == "phase_detail")
}

fn expr_mentions_phase_detail(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Field(field) => {
            matches!(&field.member, syn::Member::Named(ident) if ident == "phase_detail")
                || expr_mentions_phase_detail(&field.base)
        }
        syn::Expr::Path(path) => path_mentions_phase_detail(&path.path),
        syn::Expr::Reference(reference) => expr_mentions_phase_detail(&reference.expr),
        syn::Expr::Paren(paren) => expr_mentions_phase_detail(&paren.expr),
        syn::Expr::Tuple(tuple) => tuple.elems.iter().any(expr_mentions_phase_detail),
        syn::Expr::MethodCall(method_call) => expr_mentions_phase_detail(&method_call.receiver),
        syn::Expr::Binary(binary) => {
            expr_mentions_phase_detail(&binary.left) || expr_mentions_phase_detail(&binary.right)
        }
        _ => false,
    }
}

fn pat_mentions_phase_detail(pat: &syn::Pat) -> bool {
    match pat {
        syn::Pat::Ident(ident) => ident.ident == "phase_detail",
        syn::Pat::Reference(reference) => pat_mentions_phase_detail(&reference.pat),
        syn::Pat::Tuple(tuple) => tuple.elems.iter().any(pat_mentions_phase_detail),
        syn::Pat::TupleStruct(tuple) => tuple.elems.iter().any(pat_mentions_phase_detail),
        syn::Pat::Struct(struct_pat) => struct_pat.fields.iter().any(|field| {
            matches!(&field.member, syn::Member::Named(ident) if ident == "phase_detail")
                || pat_mentions_phase_detail(&field.pat)
        }),
        _ => false,
    }
}

struct PhaseDetailContextLiteralCollector<'a> {
    rel: &'a str,
    aliases: &'a BTreeMap<String, String>,
    values: Vec<String>,
}

impl PhaseDetailContextLiteralCollector<'_> {
    fn collect_expr(&mut self, expr: &syn::Expr) {
        collect_string_values_from_expr(self.rel, self.aliases, expr, &mut self.values);
    }
}

impl<'ast> Visit<'ast> for PhaseDetailContextLiteralCollector<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if attrs_include_test_only_cfg(&node.attrs) {
            return;
        }
        visit::visit_item_mod(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if attrs_include_test_only_cfg(&node.attrs) {
            return;
        }
        visit::visit_item_fn(self, node);
    }

    fn visit_expr_assign(&mut self, node: &'ast syn::ExprAssign) {
        if expr_mentions_phase_detail(&node.left) {
            self.collect_expr(&node.right);
        }
        visit::visit_expr_assign(self, node);
    }

    fn visit_expr_binary(&mut self, node: &'ast syn::ExprBinary) {
        if expr_mentions_phase_detail(&node.left) {
            self.collect_expr(&node.right);
        }
        if expr_mentions_phase_detail(&node.right) {
            self.collect_expr(&node.left);
        }
        visit::visit_expr_binary(self, node);
    }

    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        if expr_mentions_phase_detail(&node.expr) {
            for arm in &node.arms {
                collect_string_values_from_pat(&arm.pat, &mut self.values);
                if let Some((_if_token, guard)) = &arm.guard {
                    self.collect_expr(guard);
                }
            }
        }
        visit::visit_expr_match(self, node);
    }

    fn visit_field_value(&mut self, node: &'ast syn::FieldValue) {
        if matches!(&node.member, syn::Member::Named(ident) if ident == "phase_detail") {
            self.collect_expr(&node.expr);
        }
        visit::visit_field_value(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = node.func.as_ref()
            && path_mentions_phase_detail(&path.path)
        {
            for arg in &node.args {
                self.collect_expr(arg);
            }
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if node.method == "phase_detail" {
            for arg in &node.args {
                self.collect_expr(arg);
            }
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_local(&mut self, node: &'ast syn::Local) {
        if pat_mentions_phase_detail(&node.pat)
            && let Some(init) = &node.init
        {
            self.collect_expr(&init.expr);
        }
        visit::visit_local(self, node);
    }
}

pub fn phase_detail_context_literal_violations(
    rel: &str,
    source: &str,
    known_phase_details: &[String],
) -> Vec<String> {
    let known_phase_details = known_phase_details
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let syntax = parse_rust_source(rel, source);
    let aliases = use_aliases(rel, &syntax);
    let mut collector = PhaseDetailContextLiteralCollector {
        rel,
        aliases: &aliases,
        values: Vec::new(),
    };
    collector.visit_file(&syntax);
    collector
        .values
        .into_iter()
        .filter_map(|literal| {
            if literal == "phase_detail" {
                return None;
            }
            (is_phase_detail_shaped_literal(&literal)
                && !known_phase_details.contains(literal.as_str()))
            .then(|| {
                format!(
                    "{rel} uses unregistered phase-detail-shaped literal `{literal}` in a phase_detail context"
                )
            })
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn include_test_cfg_items_for_source(rel: &str) -> bool {
    let normalized = rel.replace('\\', "/");
    normalized.starts_with("tests/") || normalized.contains("/tests/")
}

fn should_skip_test_only_cfg(include_test_cfg_items: bool, attrs: &[syn::Attribute]) -> bool {
    !include_test_cfg_items && attrs_include_test_only_cfg(attrs)
}

struct DependencyPathCollector<'a> {
    rel: &'a str,
    aliases: &'a BTreeMap<String, String>,
    paths: Vec<String>,
}

impl<'ast> Visit<'ast> for DependencyPathCollector<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if attrs_include_test_only_cfg(&node.attrs) {
            return;
        }
        visit::visit_item_mod(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if attrs_include_test_only_cfg(&node.attrs) {
            return;
        }
        visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if attrs_include_test_only_cfg(&node.attrs) {
            return;
        }
        visit::visit_impl_item_fn(self, node);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let raw = syn_path_to_string(path);
        if !raw.is_empty() {
            self.paths
                .push(normalize_code_path_for_source(self.rel, &raw, self.aliases));
        }
        visit::visit_path(self, path);
    }

    fn visit_macro(&mut self, macro_call: &'ast syn::Macro) {
        for raw_path in macro_token_path_candidates(macro_call.tokens.clone()) {
            self.paths.push(normalize_code_path_for_source(
                self.rel,
                &raw_path,
                self.aliases,
            ));
        }
        visit::visit_macro(self, macro_call);
    }
}

pub fn normalized_code_paths(rel: &str, source: &str) -> Vec<String> {
    normalized_code_paths_with_additional_glob_aliases(rel, source, &[])
}

pub fn normalized_code_paths_with_additional_glob_aliases(
    rel: &str,
    source: &str,
    additional_glob_aliases: &[AdditionalGlobAliasSource<'_>],
) -> Vec<String> {
    let syntax = parse_rust_source(rel, source);
    let aliases = aliases_for_source(rel, source, &syntax, additional_glob_aliases);
    let mut collector = DependencyPathCollector {
        rel,
        aliases: &aliases,
        paths: Vec::new(),
    };
    collector.visit_file(&syntax);
    collector.paths.sort();
    collector.paths.dedup();
    collector.paths
}

pub fn normalized_dependency_paths(rel: &str, source: &str) -> Vec<String> {
    normalized_dependency_paths_with_additional_glob_aliases(rel, source, &[])
}

pub fn normalized_dependency_paths_with_additional_glob_aliases(
    rel: &str,
    source: &str,
    additional_glob_aliases: &[AdditionalGlobAliasSource<'_>],
) -> Vec<String> {
    let mut paths = normalized_expanded_use_paths(rel, source);
    paths.extend(normalized_code_paths_with_additional_glob_aliases(
        rel,
        source,
        additional_glob_aliases,
    ));
    paths.sort();
    paths.dedup();
    paths
}

pub fn glob_path_covers(glob_path: &str, target: &str) -> bool {
    let Some(parent) = glob_path.strip_suffix("::*") else {
        return false;
    };
    target != parent && target.starts_with(&format!("{parent}::"))
}

struct CallPathCollector<'a> {
    rel: &'a str,
    aliases: &'a BTreeMap<String, String>,
    local_alias_scopes: Vec<BTreeMap<String, Option<String>>>,
    runtime_receiver_scopes: Vec<BTreeMap<String, Option<String>>>,
    excluded_functions: BTreeSet<&'a str>,
    selected_function: Option<&'a str>,
    include_test_cfg_items: bool,
    active_depth: usize,
    calls: Vec<RustCallPath>,
}

impl CallPathCollector<'_> {
    fn should_collect_here(&self) -> bool {
        self.selected_function.is_none() || self.active_depth > 0
    }

    fn resolve_local_alias(&self, first_segment: &str) -> Option<Option<String>> {
        self.local_alias_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(first_segment).cloned())
    }

    fn resolve_runtime_receiver_alias(&self, first_segment: &str) -> Option<Option<String>> {
        self.runtime_receiver_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(first_segment).cloned())
    }

    fn normalize_call_path(&self, raw_path: &str) -> String {
        let mut segments = raw_path.split("::").filter(|segment| !segment.is_empty());
        let Some(first) = segments.next() else {
            return String::new();
        };
        if let Some(alias_target) = self.resolve_local_alias(first) {
            let Some(mut normalized) = alias_target else {
                return raw_path.to_owned();
            };
            for segment in segments {
                normalized.push_str("::");
                normalized.push_str(segment);
            }
            return normalized;
        }
        normalize_code_path_for_source(self.rel, raw_path, self.aliases)
    }

    fn expr_path_alias_target(&self, expr: &syn::Expr) -> Option<String> {
        match expr {
            syn::Expr::Path(path) => {
                Some(self.normalize_call_path(&syn_path_to_string(&path.path)))
            }
            syn::Expr::Paren(paren) => self.expr_path_alias_target(&paren.expr),
            syn::Expr::Reference(reference) => self.expr_path_alias_target(&reference.expr),
            _ => None,
        }
    }

    fn receiver_runtime_path(&self, expr: &syn::Expr) -> Option<String> {
        match expr {
            syn::Expr::Path(path) => {
                let raw_path = syn_path_to_string(&path.path);
                let mut segments = raw_path.split("::").filter(|segment| !segment.is_empty());
                let first = segments.next()?;
                let mut runtime_path = self.resolve_runtime_receiver_alias(first)??;
                for segment in segments {
                    runtime_path.push_str("::");
                    runtime_path.push_str(segment);
                }
                Some(runtime_path)
            }
            syn::Expr::Paren(paren) => self.receiver_runtime_path(&paren.expr),
            syn::Expr::Reference(reference) => self.receiver_runtime_path(&reference.expr),
            _ => None,
        }
    }

    fn receiver_path(expr: &syn::Expr) -> Option<String> {
        match expr {
            syn::Expr::Path(path) => Some(syn_path_to_string(&path.path)),
            syn::Expr::Paren(paren) => Self::receiver_path(&paren.expr),
            syn::Expr::Reference(reference) => Self::receiver_path(&reference.expr),
            _ => None,
        }
    }

    fn bind_local_aliases(&mut self, pat: &syn::Pat, alias_target: Option<String>) {
        let mut names = Vec::new();
        collect_pat_idents(pat, &mut names);
        let Some(scope) = self.local_alias_scopes.last_mut() else {
            return;
        };
        for name in names {
            let target = if matches!(pat, syn::Pat::Ident(ident) if ident.ident == name) {
                alias_target.clone()
            } else {
                None
            };
            scope.insert(name, target);
        }
    }

    fn bind_runtime_receiver_aliases(&mut self, pat: &syn::Pat, runtime_target: Option<String>) {
        let mut names = Vec::new();
        collect_pat_idents(pat, &mut names);
        let Some(scope) = self.runtime_receiver_scopes.last_mut() else {
            return;
        };
        for name in names {
            let target = if matches!(pat, syn::Pat::Ident(ident) if ident.ident == name) {
                runtime_target.clone()
            } else {
                None
            };
            scope.insert(name, target);
        }
    }

    fn type_is_execution_runtime(&self, ty: &syn::Type) -> bool {
        match ty {
            syn::Type::Reference(reference) => self.type_is_execution_runtime(&reference.elem),
            syn::Type::Paren(paren) => self.type_is_execution_runtime(&paren.elem),
            syn::Type::Group(group) => self.type_is_execution_runtime(&group.elem),
            syn::Type::Path(path) => {
                let raw_path = syn_path_to_string(&path.path);
                let normalized = normalize_code_path_for_source(self.rel, &raw_path, self.aliases);
                normalized == "crate::execution::state::ExecutionRuntime"
                    || normalized == "featureforge::execution::state::ExecutionRuntime"
                    || normalized.rsplit("::").next() == Some("ExecutionRuntime")
            }
            _ => false,
        }
    }

    fn bind_runtime_receiver_params(&mut self, sig: &syn::Signature) {
        let mut runtime_names = Vec::new();
        for input in &sig.inputs {
            let syn::FnArg::Typed(pat_type) = input else {
                continue;
            };
            if !self.type_is_execution_runtime(&pat_type.ty) {
                continue;
            }
            collect_pat_idents(&pat_type.pat, &mut runtime_names);
        }
        let Some(scope) = self.runtime_receiver_scopes.last_mut() else {
            return;
        };
        for name in runtime_names {
            scope.insert(name.clone(), Some(name));
        }
    }

    fn visit_scoped_block(&mut self, block: &syn::Block) {
        self.local_alias_scopes.push(BTreeMap::new());
        self.runtime_receiver_scopes.push(BTreeMap::new());
        visit::visit_block(self, block);
        self.runtime_receiver_scopes.pop();
        self.local_alias_scopes.pop();
    }

    fn record_path(&mut self, raw: &str, line: usize) {
        if self.should_collect_here() {
            self.calls.push(RustCallPath {
                path: self.normalize_call_path(raw),
                raw_path: raw.to_owned(),
                receiver_path: None,
                receiver_runtime_path: None,
                line,
            });
        }
    }
}

impl<'ast> Visit<'ast> for CallPathCollector<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if should_skip_test_only_cfg(self.include_test_cfg_items, &node.attrs) {
            return;
        }
        visit::visit_item_mod(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if should_skip_test_only_cfg(self.include_test_cfg_items, &node.attrs)
            || self
                .excluded_functions
                .contains(node.sig.ident.to_string().as_str())
        {
            return;
        }
        let selected = self
            .selected_function
            .is_none_or(|function| node.sig.ident == function);
        if selected {
            self.runtime_receiver_scopes.push(BTreeMap::new());
            self.bind_runtime_receiver_params(&node.sig);
            self.active_depth += 1;
            self.visit_scoped_block(&node.block);
            self.active_depth -= 1;
            self.runtime_receiver_scopes.pop();
        }
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if should_skip_test_only_cfg(self.include_test_cfg_items, &node.attrs)
            || self
                .excluded_functions
                .contains(node.sig.ident.to_string().as_str())
        {
            return;
        }
        let selected = self
            .selected_function
            .is_none_or(|function| node.sig.ident == function);
        if selected {
            self.runtime_receiver_scopes.push(BTreeMap::new());
            self.bind_runtime_receiver_params(&node.sig);
            self.active_depth += 1;
            self.visit_scoped_block(&node.block);
            self.active_depth -= 1;
            self.runtime_receiver_scopes.pop();
        }
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = node.func.as_ref() {
            self.record_path(
                &syn_path_to_string(&path.path),
                path.path.segments.span().start().line,
            );
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if self.should_collect_here() {
            self.calls.push(RustCallPath {
                path: node.method.to_string(),
                raw_path: node.method.to_string(),
                receiver_path: Self::receiver_path(&node.receiver),
                receiver_runtime_path: self.receiver_runtime_path(&node.receiver),
                line: node.method.span().start().line,
            });
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_block(&mut self, node: &'ast syn::Block) {
        self.visit_scoped_block(node);
    }

    fn visit_local(&mut self, node: &'ast syn::Local) {
        let alias_target = node
            .init
            .as_ref()
            .and_then(|init| self.expr_path_alias_target(&init.expr));
        let runtime_target = node
            .init
            .as_ref()
            .and_then(|init| self.receiver_runtime_path(&init.expr));
        visit::visit_local(self, node);
        self.bind_local_aliases(&node.pat, alias_target);
        self.bind_runtime_receiver_aliases(&node.pat, runtime_target);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if self.should_collect_here() {
            for raw_path in macro_token_path_candidates(node.tokens.clone()) {
                self.calls.push(RustCallPath {
                    path: self.normalize_call_path(&raw_path),
                    raw_path,
                    receiver_path: None,
                    receiver_runtime_path: None,
                    line: node.path.span().start().line,
                });
            }
        }
        visit::visit_macro(self, node);
    }
}

fn collect_pat_idents(pat: &syn::Pat, names: &mut Vec<String>) {
    match pat {
        syn::Pat::Ident(ident) => names.push(ident.ident.to_string()),
        syn::Pat::Reference(reference) => collect_pat_idents(&reference.pat, names),
        syn::Pat::Tuple(tuple) => {
            for elem in &tuple.elems {
                collect_pat_idents(elem, names);
            }
        }
        syn::Pat::TupleStruct(tuple) => {
            for elem in &tuple.elems {
                collect_pat_idents(elem, names);
            }
        }
        syn::Pat::Struct(struct_pat) => {
            for field in &struct_pat.fields {
                collect_pat_idents(&field.pat, names);
            }
        }
        syn::Pat::Slice(slice) => {
            for elem in &slice.elems {
                collect_pat_idents(elem, names);
            }
        }
        syn::Pat::Or(or_pat) => {
            for case in &or_pat.cases {
                collect_pat_idents(case, names);
            }
        }
        syn::Pat::Paren(paren) => collect_pat_idents(&paren.pat, names),
        syn::Pat::Type(typed) => collect_pat_idents(&typed.pat, names),
        syn::Pat::Const(_)
        | syn::Pat::Lit(_)
        | syn::Pat::Macro(_)
        | syn::Pat::Path(_)
        | syn::Pat::Range(_)
        | syn::Pat::Rest(_)
        | syn::Pat::Verbatim(_)
        | syn::Pat::Wild(_) => {}
        _ => {}
    }
}

pub fn normalized_call_paths(rel: &str, source: &str, excluded_functions: &[&str]) -> Vec<String> {
    normalized_call_paths_for_selection(rel, source, excluded_functions, None)
}

pub fn normalized_call_paths_in_function(
    rel: &str,
    source: &str,
    function_name: &str,
) -> Vec<String> {
    normalized_call_paths_for_selection(rel, source, &[], Some(function_name))
}

fn normalized_call_paths_for_selection(
    rel: &str,
    source: &str,
    excluded_functions: &[&str],
    selected_function: Option<&str>,
) -> Vec<String> {
    let syntax = parse_rust_source(rel, source);
    let aliases = use_aliases(rel, &syntax);
    let mut collector = CallPathCollector {
        rel,
        aliases: &aliases,
        local_alias_scopes: vec![BTreeMap::new()],
        runtime_receiver_scopes: vec![BTreeMap::new()],
        excluded_functions: excluded_functions.iter().copied().collect(),
        selected_function,
        include_test_cfg_items: include_test_cfg_items_for_source(rel),
        active_depth: 0,
        calls: Vec::new(),
    };
    collector.visit_file(&syntax);
    let mut paths = collector
        .calls
        .into_iter()
        .map(|call| call.path)
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustCallPath {
    pub path: String,
    pub raw_path: String,
    pub receiver_path: Option<String>,
    pub receiver_runtime_path: Option<String>,
    pub line: usize,
}

pub fn normalized_call_path_hits(
    rel: &str,
    source: &str,
    excluded_functions: &[&str],
) -> Vec<RustCallPath> {
    let syntax = parse_rust_source(rel, source);
    let aliases = use_aliases(rel, &syntax);
    let mut collector = CallPathCollector {
        rel,
        aliases: &aliases,
        local_alias_scopes: vec![BTreeMap::new()],
        runtime_receiver_scopes: vec![BTreeMap::new()],
        excluded_functions: excluded_functions.iter().copied().collect(),
        selected_function: None,
        include_test_cfg_items: include_test_cfg_items_for_source(rel),
        active_depth: 0,
        calls: Vec::new(),
    };
    collector.visit_file(&syntax);
    collector.calls.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then_with(|| left.path.cmp(&right.path))
    });
    collector.calls.dedup();
    collector.calls
}

#[derive(Debug)]
pub struct RustWriterCall {
    pub function: String,
    pub callee: String,
    pub target_arg: Option<String>,
}

struct WriterCallCollector<'a, IsWriter, TargetArg>
where
    IsWriter: Fn(&str) -> bool,
    TargetArg: Fn(&str, &syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>) -> Option<String>,
{
    rel: &'a str,
    aliases: &'a BTreeMap<String, String>,
    is_writer_call: &'a IsWriter,
    writer_target_arg_name: &'a TargetArg,
    local_alias_scopes: Vec<BTreeMap<String, Option<String>>>,
    function_stack: Vec<String>,
    calls: Vec<RustWriterCall>,
}

impl<IsWriter, TargetArg> WriterCallCollector<'_, IsWriter, TargetArg>
where
    IsWriter: Fn(&str) -> bool,
    TargetArg: Fn(&str, &syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>) -> Option<String>,
{
    fn current_function(&self) -> String {
        self.function_stack
            .last()
            .cloned()
            .unwrap_or_else(|| String::from("<module>"))
    }

    fn resolve_local_alias(&self, first_segment: &str) -> Option<Option<String>> {
        self.local_alias_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(first_segment).cloned())
    }

    fn normalize_writer_path(&self, raw_path: &str) -> String {
        let mut segments = raw_path.split("::").filter(|segment| !segment.is_empty());
        let Some(first) = segments.next() else {
            return String::new();
        };
        if let Some(alias_target) = self.resolve_local_alias(first) {
            let Some(mut normalized) = alias_target else {
                return raw_path.to_owned();
            };
            for segment in segments {
                normalized.push_str("::");
                normalized.push_str(segment);
            }
            return normalized;
        }
        normalize_code_path_for_source(self.rel, raw_path, self.aliases)
    }

    fn writer_alias_target(&self, expr: &syn::Expr) -> Option<String> {
        match expr {
            syn::Expr::Path(path) => {
                let raw = syn_path_to_string(&path.path);
                let target = self.normalize_writer_path(&raw);
                (self.is_writer_call)(&target).then_some(target)
            }
            syn::Expr::Paren(paren) => self.writer_alias_target(&paren.expr),
            syn::Expr::Reference(reference) => self.writer_alias_target(&reference.expr),
            _ => None,
        }
    }

    fn bind_local_aliases(&mut self, pat: &syn::Pat, writer_target: Option<String>) {
        let mut names = Vec::new();
        collect_pat_idents(pat, &mut names);
        let Some(scope) = self.local_alias_scopes.last_mut() else {
            return;
        };
        for name in names {
            let target = if matches!(pat, syn::Pat::Ident(ident) if ident.ident == name) {
                writer_target.clone()
            } else {
                None
            };
            scope.insert(name, target);
        }
    }

    fn record_writer_alias_declaration(&mut self, alias_name: String, writer_target: String) {
        self.calls.push(RustWriterCall {
            function: format!("alias {alias_name}"),
            callee: writer_target.clone(),
            target_arg: None,
        });
        if let Some(scope) = self.local_alias_scopes.last_mut() {
            scope.insert(alias_name, Some(writer_target));
        }
    }

    fn visit_scoped_block(&mut self, block: &syn::Block) {
        self.local_alias_scopes.push(BTreeMap::new());
        visit::visit_block(self, block);
        self.local_alias_scopes.pop();
    }

    fn record_path_call(
        &mut self,
        path: &syn::Path,
        args: &syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>,
    ) {
        let raw_callee = syn_path_to_string(path);
        let callee = self.normalize_writer_path(&raw_callee);
        if (self.is_writer_call)(&callee) {
            let target_arg = (self.writer_target_arg_name)(&callee, args);
            self.calls.push(RustWriterCall {
                function: self.current_function(),
                callee,
                target_arg,
            });
        }
    }
}

impl<'ast, IsWriter, TargetArg> Visit<'ast> for WriterCallCollector<'_, IsWriter, TargetArg>
where
    IsWriter: Fn(&str) -> bool,
    TargetArg: Fn(&str, &syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>) -> Option<String>,
{
    fn visit_item_macro(&mut self, node: &'ast syn::ItemMacro) {
        if attrs_include_test_only_cfg(&node.attrs) {
            return;
        }
        visit::visit_item_macro(self, node);
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if attrs_include_test_only_cfg(&item.attrs) {
            return;
        }
        visit::visit_item_mod(self, item);
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if attrs_include_test_only_cfg(&item.attrs) {
            return;
        }
        self.function_stack.push(item.sig.ident.to_string());
        self.visit_scoped_block(&item.block);
        self.function_stack.pop();
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if attrs_include_test_only_cfg(&item.attrs) {
            return;
        }
        self.function_stack.push(item.sig.ident.to_string());
        self.visit_scoped_block(&item.block);
        self.function_stack.pop();
    }

    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        if attrs_include_test_only_cfg(&node.attrs) {
            return;
        }
        if let Some(writer_target) = self.writer_alias_target(&node.expr) {
            self.record_writer_alias_declaration(node.ident.to_string(), writer_target);
        }
        visit::visit_item_const(self, node);
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        if attrs_include_test_only_cfg(&node.attrs) {
            return;
        }
        if let Some(writer_target) = self.writer_alias_target(&node.expr) {
            self.record_writer_alias_declaration(node.ident.to_string(), writer_target);
        }
        visit::visit_item_static(self, node);
    }

    fn visit_impl_item_const(&mut self, node: &'ast syn::ImplItemConst) {
        if attrs_include_test_only_cfg(&node.attrs) {
            return;
        }
        if let Some(writer_target) = self.writer_alias_target(&node.expr) {
            self.record_writer_alias_declaration(node.ident.to_string(), writer_target);
        }
        visit::visit_impl_item_const(self, node);
    }

    fn visit_block(&mut self, node: &'ast syn::Block) {
        self.visit_scoped_block(node);
    }

    fn visit_local(&mut self, node: &'ast syn::Local) {
        let writer_target = node
            .init
            .as_ref()
            .and_then(|init| self.writer_alias_target(&init.expr));
        visit::visit_local(self, node);
        self.bind_local_aliases(&node.pat, writer_target);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = node.func.as_ref() {
            self.record_path_call(&path.path, &node.args);
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let callee = node.method.to_string();
        if (self.is_writer_call)(&callee) {
            self.calls.push(RustWriterCall {
                function: self.current_function(),
                callee,
                target_arg: None,
            });
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        for raw_path in macro_token_path_candidates(node.tokens.clone()) {
            let callee = self.normalize_writer_path(&raw_path);
            if (self.is_writer_call)(&callee) {
                self.calls.push(RustWriterCall {
                    function: format!("macro in {}", self.current_function()),
                    callee,
                    target_arg: None,
                });
            }
        }
        visit::visit_macro(self, node);
    }
}

pub fn writer_call_hits<IsWriter, TargetArg>(
    rel: &str,
    source: &str,
    additional_glob_aliases: &[AdditionalGlobAliasSource<'_>],
    is_writer_call: IsWriter,
    writer_target_arg_name: TargetArg,
) -> Vec<RustWriterCall>
where
    IsWriter: Fn(&str) -> bool,
    TargetArg: Fn(&str, &syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>) -> Option<String>,
{
    let syntax = parse_rust_source(rel, source);
    let aliases = aliases_for_source(rel, source, &syntax, additional_glob_aliases);
    let mut collector = WriterCallCollector {
        rel,
        aliases: &aliases,
        is_writer_call: &is_writer_call,
        writer_target_arg_name: &writer_target_arg_name,
        local_alias_scopes: vec![BTreeMap::new()],
        function_stack: Vec::new(),
        calls: Vec::new(),
    };
    collector.visit_file(&syntax);
    collector.calls
}

struct FunctionNameCollector {
    names: Vec<String>,
}

impl<'ast> Visit<'ast> for FunctionNameCollector {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if attrs_include_test_only_cfg(&node.attrs) {
            return;
        }
        visit::visit_item_mod(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if attrs_include_test_only_cfg(&node.attrs) {
            return;
        }
        self.names.push(node.sig.ident.to_string());
        visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if attrs_include_test_only_cfg(&node.attrs) {
            return;
        }
        self.names.push(node.sig.ident.to_string());
        visit::visit_impl_item_fn(self, node);
    }
}

pub fn function_names(rel: &str, source: &str) -> Vec<String> {
    let syntax = parse_rust_source(rel, source);
    let mut collector = FunctionNameCollector { names: Vec::new() };
    collector.visit_file(&syntax);
    collector.names.sort();
    collector.names.dedup();
    collector.names
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustFunctionSpan {
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
}

struct FunctionSpanCollector {
    include_test_cfg_items: bool,
    spans: Vec<RustFunctionSpan>,
}

impl FunctionSpanCollector {
    fn record_function(&mut self, name: String, attrs: &[syn::Attribute], block: &syn::Block) {
        if should_skip_test_only_cfg(self.include_test_cfg_items, attrs) {
            return;
        }
        let start_line = block.brace_token.span.open().start().line;
        let end_line = block.brace_token.span.close().end().line;
        self.spans.push(RustFunctionSpan {
            name,
            start_line,
            end_line,
        });
    }
}

impl<'ast> Visit<'ast> for FunctionSpanCollector {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if should_skip_test_only_cfg(self.include_test_cfg_items, &node.attrs) {
            return;
        }
        visit::visit_item_mod(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.record_function(node.sig.ident.to_string(), &node.attrs, &node.block);
        visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.record_function(node.sig.ident.to_string(), &node.attrs, &node.block);
        visit::visit_impl_item_fn(self, node);
    }
}

pub fn function_spans(rel: &str, source: &str) -> Vec<RustFunctionSpan> {
    let syntax = parse_rust_source(rel, source);
    let mut collector = FunctionSpanCollector {
        include_test_cfg_items: include_test_cfg_items_for_source(rel),
        spans: Vec::new(),
    };
    collector.visit_file(&syntax);
    collector.spans.sort_by(|left, right| {
        left.start_line
            .cmp(&right.start_line)
            .then_with(|| left.end_line.cmp(&right.end_line))
            .then_with(|| left.name.cmp(&right.name))
    });
    collector.spans
}

pub fn forbidden_call_violations(
    rel: &str,
    source: &str,
    forbidden_function_names: &[&str],
    excluded_functions: &[&str],
) -> Vec<String> {
    forbidden_calls_from_paths(
        rel,
        &normalized_call_paths(rel, source, excluded_functions),
        forbidden_function_names,
    )
}

pub fn forbidden_call_violations_in_function(
    rel: &str,
    source: &str,
    function_name: &str,
    forbidden_function_names: &[&str],
) -> Vec<String> {
    forbidden_calls_from_paths(
        rel,
        &normalized_call_paths_in_function(rel, source, function_name),
        forbidden_function_names,
    )
    .into_iter()
    .map(|violation| format!("{rel}::{function_name} {violation}"))
    .collect()
}

fn forbidden_calls_from_paths(
    rel: &str,
    paths: &[String],
    forbidden_function_names: &[&str],
) -> Vec<String> {
    let forbidden = forbidden_function_names
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    paths
        .iter()
        .filter_map(|path| {
            let leaf = path.rsplit("::").next().unwrap_or(path.as_str());
            forbidden
                .contains(leaf)
                .then(|| format!("{rel} calls `{path}`"))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
