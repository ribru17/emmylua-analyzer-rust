use emmylua_code_analysis::{
    InFiled, LuaFunctionType, LuaInstanceType, LuaOperatorMetaMethod, LuaOperatorOwner,
    LuaSemanticDeclId, LuaSignatureId, LuaType, LuaTypeDeclId, RenderLevel, SemanticModel,
};
use emmylua_parser::{LuaAstNode, LuaCallExpr, LuaSyntaxToken, LuaTokenKind};
use lsp_types::{
    Documentation, MarkupContent, ParameterInformation, ParameterLabel, SignatureHelp,
    SignatureInformation,
};
use rowan::{NodeOrToken, TextRange};

use emmylua_code_analysis::humanize_type;

use super::signature_helper_builder::SignatureHelperBuilder;

pub fn build_signature_helper(
    semantic_model: &SemanticModel,
    call_expr: LuaCallExpr,
    token: LuaSyntaxToken,
) -> Option<SignatureHelp> {
    let prefix_expr = call_expr.get_prefix_expr()?;
    let prefix_expr_type = semantic_model.infer_expr(prefix_expr.clone()).ok()?;
    let builder = SignatureHelperBuilder::new(semantic_model, call_expr.clone());
    let colon_call = call_expr.is_colon_call();
    let current_idx = get_current_param_index(&call_expr, &token)?;
    match prefix_expr_type {
        LuaType::DocFunction(func_type) => {
            build_doc_function_signature_help(&builder, &func_type, colon_call, current_idx)
        }
        LuaType::Signature(signature_id) => {
            build_sig_id_signature_help(&builder, signature_id, colon_call, current_idx, false)
        }
        LuaType::Ref(type_decl_id) => {
            build_type_signature_help(&builder, &type_decl_id, colon_call, current_idx)
        }
        LuaType::Def(type_decl_id) => {
            build_type_signature_help(&builder, &type_decl_id, colon_call, current_idx)
        }
        LuaType::Instance(inst) => {
            build_inst_signature_help(&builder, &inst, colon_call, current_idx)
        }
        LuaType::TableConst(meta_table) => {
            build_table_call_signature_help(&builder, meta_table, colon_call, current_idx)
        }
        LuaType::Union(union_types) => build_union_type_signature_help(
            &builder,
            union_types.get_types(),
            colon_call,
            current_idx,
        ),
        _ => None,
    }
}

pub fn get_current_param_index(call_expr: &LuaCallExpr, token: &LuaSyntaxToken) -> Option<usize> {
    let arg_list = call_expr.get_args_list()?;
    let mut current_idx = 0;
    let token_position = token.text_range().start();
    for node_or_token in arg_list.syntax().children_with_tokens() {
        if let NodeOrToken::Token(token) = node_or_token {
            if token.kind() == LuaTokenKind::TkComma.into() {
                if token.text_range().start() <= token_position {
                    current_idx += 1;
                }
            }
        }
    }

    Some(current_idx)
}

fn build_doc_function_signature_help(
    builder: &SignatureHelperBuilder,
    func_type: &LuaFunctionType,
    colon_call: bool,
    current_idx: usize,
) -> Option<SignatureHelp> {
    let semantic_model = builder.semantic_model;
    let db = semantic_model.get_db();
    let mut current_idx = current_idx;
    let params = func_type
        .get_params()
        .iter()
        .map(|param| param.clone())
        .collect::<Vec<_>>();
    // 参数信息
    let mut param_infos = vec![];
    for param in params.iter() {
        let param_name = param.0.clone();
        let param_type = param.1.clone();
        let param_label = format!(
            "{}: {}",
            param_name,
            humanize_type(db, &param_type.unwrap_or(LuaType::Any), RenderLevel::Simple)
        );

        param_infos.push(ParameterInformation {
            label: ParameterLabel::Simple(param_label),
            documentation: None,
        });
    }

    let colon_define = func_type.is_colon_define();
    match (colon_define, colon_call) {
        (true, false) => {
            let self_type = builder.get_self_type();
            if let Some(self_type) = self_type {
                let self_label = format!(
                    "self: {}",
                    humanize_type(db, &self_type, RenderLevel::Simple)
                );
                param_infos.insert(
                    0,
                    ParameterInformation {
                        label: ParameterLabel::Simple(self_label),
                        documentation: None,
                    },
                );
            }
        }
        (false, true) => {
            if !param_infos.is_empty() {
                param_infos.remove(0);
            }
        }
        _ => {}
    }

    if let Some((name, _)) = params.last() {
        if name == "..." && current_idx >= params.len() {
            current_idx = params.len() - 1;
        }
    }

    let label = build_function_label(
        builder,
        &param_infos,
        func_type.is_colon_define() || func_type.first_param_is_self(),
        &func_type.get_ret(),
    );

    let signature_info = SignatureInformation {
        label,
        documentation: None,
        parameters: Some(param_infos),
        active_parameter: Some(current_idx as u32),
    };

    Some(SignatureHelp {
        signatures: vec![signature_info],
        active_signature: Some(0),
        active_parameter: Some(current_idx as u32),
    })
}

fn build_sig_id_signature_help(
    builder: &SignatureHelperBuilder,
    signature_id: LuaSignatureId,
    colon_call: bool,
    current_idx: usize,
    is_call_operator: bool,
) -> Option<SignatureHelp> {
    let semantic_model = builder.semantic_model;
    let origin_current_idx = current_idx;
    let db = semantic_model.get_db();
    let signature = db.get_signature_index().get(&signature_id)?;
    let mut current_idx = current_idx;
    let mut params = signature.get_type_params();
    let colon_define = signature.is_colon_define;
    if is_call_operator {
        if params.len() > 0 && !colon_define {
            params.remove(0);
        }
    }
    // 参数信息
    let mut param_infos = vec![];
    for param in params.iter() {
        let param_name = param.0.clone();
        let param_type = param.1.clone();
        let param_label = format!(
            "{}: {}",
            param_name,
            humanize_type(db, &param_type.unwrap_or(LuaType::Any), RenderLevel::Simple)
        );

        let mut documentation_string = String::new();
        if let Some(desc) = signature.get_param_info_by_name(&param_name) {
            if let Some(description) = &desc.description {
                documentation_string.push_str(description);
            }
        }

        let documentation = if documentation_string.is_empty() {
            None
        } else {
            Some(Documentation::MarkupContent(MarkupContent {
                kind: lsp_types::MarkupKind::Markdown,
                value: documentation_string,
            }))
        };

        param_infos.push(ParameterInformation {
            label: ParameterLabel::Simple(param_label),
            documentation,
        });
    }

    match (colon_define, colon_call) {
        (true, false) => {
            let self_type = builder.get_self_type();
            if let Some(self_type) = self_type {
                let self_label = format!(
                    "self: {}",
                    humanize_type(db, &self_type, RenderLevel::Simple)
                );
                param_infos.insert(
                    0,
                    ParameterInformation {
                        label: ParameterLabel::Simple(self_label),
                        documentation: None,
                    },
                );
            }
        }
        (false, true) => {
            if !param_infos.is_empty() {
                param_infos.remove(0);
            }
        }
        _ => {}
    }

    if let Some((name, _)) = params.last() {
        if name == "..." && current_idx >= params.len() {
            current_idx = params.len() - 1;
        }
    }

    let label = build_function_label(
        builder,
        &param_infos,
        signature.is_method(),
        &signature.get_return_types(),
    );

    let documentation = build_documentation(builder, signature_id);

    let signature_info = SignatureInformation {
        label,
        documentation,
        parameters: Some(param_infos),
        active_parameter: Some(current_idx as u32),
    };

    let mut signatures = vec![signature_info];
    for overload in &signature.overloads {
        let signature =
            build_doc_function_signature_help(&builder, &overload, colon_call, origin_current_idx);
        if let Some(mut signature) = signature {
            signature.signatures[0].documentation = build_documentation(builder, signature_id);
            signatures.push(signature.signatures[0].clone());
        }
    }

    Some(SignatureHelp {
        signatures,
        active_signature: Some(0),
        active_parameter: Some(current_idx as u32),
    })
}

// todo support overload
fn build_type_signature_help(
    builder: &SignatureHelperBuilder,
    type_decl_id: &LuaTypeDeclId,
    colon_call: bool,
    current_idx: usize,
) -> Option<SignatureHelp> {
    let db = builder.semantic_model.get_db();
    let operator_ids = db
        .get_operator_index()
        .get_operators(&type_decl_id.clone().into(), LuaOperatorMetaMethod::Call)?;

    for operator_id in operator_ids {
        let operator = db.get_operator_index().get_operator(operator_id)?;
        let call_type = operator.get_operator_func();
        match call_type {
            LuaType::DocFunction(func_type) => {
                return build_doc_function_signature_help(
                    builder,
                    &func_type,
                    colon_call,
                    current_idx,
                );
            }
            LuaType::Signature(signature_id) => {
                // todo remove first param
                return build_sig_id_signature_help(
                    builder,
                    signature_id,
                    colon_call,
                    current_idx,
                    true,
                );
            }
            _ => {}
        }
    }

    None
}

fn build_inst_signature_help(
    builder: &SignatureHelperBuilder,
    inst: &LuaInstanceType,
    colon_call: bool,
    current_idx: usize,
) -> Option<SignatureHelp> {
    let base = inst.get_base();
    let meta_table = match base {
        LuaType::TableConst(meta_table) => meta_table.clone(),
        _ => {
            return None;
        }
    };

    build_table_call_signature_help(builder, meta_table, colon_call, current_idx)
}

fn build_table_call_signature_help(
    builder: &SignatureHelperBuilder,
    meta: InFiled<TextRange>,
    colon_call: bool,
    current_idx: usize,
) -> Option<SignatureHelp> {
    let semantic_model = builder.semantic_model;
    let metatable = semantic_model.get_db().get_metatable_index().get(&meta)?;

    let operator_owner = LuaOperatorOwner::Table(metatable.clone());
    let db = semantic_model.get_db();
    let operator_ids = db
        .get_operator_index()
        .get_operators(&operator_owner, LuaOperatorMetaMethod::Call)?
        .first()?;
    let operator = db.get_operator_index().get_operator(operator_ids)?;
    let call_type = operator.get_operator_func();
    match call_type {
        LuaType::DocFunction(func_type) => {
            return build_doc_function_signature_help(builder, &func_type, colon_call, current_idx);
        }
        LuaType::Signature(signature_id) => {
            return build_sig_id_signature_help(
                builder,
                signature_id,
                colon_call,
                current_idx,
                true,
            );
        }
        _ => {}
    }

    None
}

fn build_union_type_signature_help(
    builder: &SignatureHelperBuilder,
    union_types: &[LuaType],
    colon_call: bool,
    current_idx: usize,
) -> Option<SignatureHelp> {
    let mut signatures = vec![];
    let active_parameter = current_idx as u32;
    for typ in union_types {
        match typ {
            LuaType::DocFunction(func_type) => {
                let sig =
                    build_doc_function_signature_help(builder, &func_type, colon_call, current_idx);

                if let Some(sig) = sig {
                    signatures.push(sig.signatures[0].clone());
                }
            }
            LuaType::Signature(signature_id) => {
                let sig = build_sig_id_signature_help(
                    builder,
                    *signature_id,
                    colon_call,
                    current_idx,
                    false,
                );

                if let Some(sig) = sig {
                    signatures.extend(sig.signatures);
                }
            }
            LuaType::Ref(type_decl_id) => {
                let sig =
                    build_type_signature_help(builder, &type_decl_id, colon_call, current_idx);

                if let Some(sig) = sig {
                    signatures.extend(sig.signatures);
                }
            }
            LuaType::Def(type_decl_id) => {
                let sig =
                    build_type_signature_help(builder, &type_decl_id, colon_call, current_idx);

                if let Some(sig) = sig {
                    signatures.extend(sig.signatures);
                }
            }
            _ => {}
        }
    }

    Some(SignatureHelp {
        signatures,
        active_signature: Some(0),
        active_parameter: Some(active_parameter),
    })
}

fn build_function_label(
    builder: &SignatureHelperBuilder,
    param_infos: &[ParameterInformation],
    is_method: bool,
    return_types: &[LuaType],
) -> String {
    let mut label = String::new();
    if let Some(prefix_name) = &builder.prefix_name {
        label.push_str(prefix_name);
        if is_method {
            label.push_str(":");
        } else {
            label.push_str(".");
        }
    }
    label.push_str(&builder.function_name);
    label.push_str("(");
    label.push_str(
        &param_infos
            .iter()
            .map(|info| match &info.label {
                ParameterLabel::Simple(label) => label.clone(),
                ParameterLabel::LabelOffsets(_) => todo!(),
            })
            .collect::<Vec<_>>()
            .join(", "),
    );
    label.push_str(")");
    // 返回值
    if !return_types.is_empty() {
        label.push_str(": ");
        for return_type in return_types {
            label.push_str(&humanize_type(
                builder.semantic_model.get_db(),
                &return_type,
                RenderLevel::Simple,
            ));
            label.push_str(",");
        }
        label.pop();
    }

    label
}

/// 生成评论信息
fn build_documentation(
    builder: &SignatureHelperBuilder,
    signature_id: LuaSignatureId,
) -> Option<Documentation> {
    let db = builder.semantic_model.get_db();
    let property_owner = LuaSemanticDeclId::Signature(signature_id);
    let documentation =
        if let Some(property) = db.get_property_index().get_property(&property_owner) {
            if let Some(description) = &property.description {
                Some(Documentation::MarkupContent(MarkupContent {
                    kind: lsp_types::MarkupKind::Markdown,
                    value: description.to_string(),
                }))
            } else {
                None
            }
        } else {
            None
        };
    documentation
}
