use emmylua_parser::{
    LuaAstNode, LuaIndexExpr, LuaIndexKey, LuaIndexMemberExpr, LuaSyntaxId, LuaSyntaxKind,
    LuaTableExpr, PathTrait,
};
use rowan::TextRange;
use smol_str::SmolStr;

use crate::{
    db_index::{
        DbIndex, LuaGenericType, LuaIntersectionType, LuaMemberKey, LuaObjectType,
        LuaOperatorMetaMethod, LuaTupleType, LuaType, LuaTypeDeclId, LuaUnionType,
    },
    semantic::{
        generic::{instantiate_type_generic, TypeSubstitutor},
        member::{get_buildin_type_map_type_id, without_index_operator},
        type_check::check_type_compact,
        InferGuard,
    },
    InFiled, LuaFlowId, LuaInferCache, LuaInstanceType, LuaMemberOwner,
};

use super::{infer_expr, InferResult};

pub fn infer_index_expr(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    index_expr: LuaIndexExpr,
) -> InferResult {
    let prefix_expr = index_expr.get_prefix_expr()?;
    let prefix_type = infer_expr(db, cache, prefix_expr)?;
    let index_member_expr = LuaIndexMemberExpr::IndexExpr(index_expr.clone());

    let mut member_type = if let Some(member_type) = infer_member_by_member_key(
        db,
        cache,
        &prefix_type,
        index_member_expr.clone(),
        &mut InferGuard::new(),
    ) {
        member_type
    } else if let Some(member_type) = infer_member_by_operator(
        db,
        cache,
        &prefix_type,
        index_member_expr,
        &mut InferGuard::new(),
    ) {
        member_type
    } else if cache.get_config().analysis_phase.is_force() {
        LuaType::Unknown
    } else {
        return None;
    };

    // 临时修复, 应该处理 flow
    // TODO: flow 分析时若前置类型是数组, 则不应生成对应的`flow_chain`
    match &prefix_type {
        LuaType::Array(_) => {
            return Some(member_type.clone());
        }
        _ => {}
    }

    let flow_id = LuaFlowId::from_node(index_expr.syntax());
    let flow_chain = db
        .get_flow_index()
        .get_flow_chain(cache.get_file_id(), flow_id);
    if let Some(flow_chain) = flow_chain {
        let root = index_expr.get_root();
        if let Some(path) = index_expr.get_access_path() {
            for type_assert in flow_chain.get_type_asserts(&path, index_expr.get_position(), None) {
                member_type = type_assert
                    .tighten_type(db, cache, &root, member_type)
                    .unwrap_or(LuaType::Unknown);
            }
        }
    }

    Some(member_type)
}

pub fn infer_member_by_member_key(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    prefix_type: &LuaType,
    index_expr: LuaIndexMemberExpr,
    infer_guard: &mut InferGuard,
) -> InferResult {
    match &prefix_type {
        LuaType::Table | LuaType::Any | LuaType::Unknown => Some(LuaType::Any),
        LuaType::TableConst(id) => infer_table_member(db, id.clone(), index_expr),
        LuaType::String | LuaType::Io | LuaType::StringConst(_) | LuaType::DocStringConst(_) => {
            let decl_id = get_buildin_type_map_type_id(&prefix_type)?;
            infer_custom_type_member(db, cache, decl_id, index_expr, infer_guard)
        }
        LuaType::Ref(decl_id) => {
            infer_custom_type_member(db, cache, decl_id.clone(), index_expr, infer_guard)
        }
        LuaType::Def(decl_id) => {
            infer_custom_type_member(db, cache, decl_id.clone(), index_expr, infer_guard)
        }
        // LuaType::Module(_) => todo!(),
        LuaType::Tuple(tuple_type) => infer_tuple_member(tuple_type, index_expr),
        LuaType::Object(object_type) => infer_object_member(db, cache, object_type, index_expr),
        LuaType::Union(union_type) => infer_union_member(db, cache, union_type, index_expr),
        LuaType::Intersection(intersection_type) => {
            infer_intersection_member(db, cache, intersection_type, index_expr)
        }
        LuaType::Generic(generic_type) => infer_generic_member(db, cache, generic_type, index_expr),
        LuaType::Global => infer_global_field_member(db, cache, index_expr),
        LuaType::Instance(inst) => infer_instance_member(db, cache, inst, index_expr, infer_guard),
        LuaType::Namespace(ns) => infer_namespace_member(db, cache, ns, index_expr),
        _ => None,
    }
}

fn infer_table_member(
    db: &DbIndex,
    inst: InFiled<TextRange>,
    index_expr: LuaIndexMemberExpr,
) -> InferResult {
    let owner = LuaMemberOwner::Element(inst);
    let key: LuaMemberKey = index_expr.get_index_key()?.into();
    let member_item = db.get_member_index().get_member_item(&owner, &key)?;
    member_item.resolve_type(db)
}

fn infer_custom_type_member(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    prefix_type_id: LuaTypeDeclId,
    index_expr: LuaIndexMemberExpr,
    infer_guard: &mut InferGuard,
) -> InferResult {
    infer_guard.check(&prefix_type_id)?;
    let type_index = db.get_type_index();
    let type_decl = type_index.get_type_decl(&prefix_type_id)?;
    if type_decl.is_alias() {
        if let Some(origin_type) = type_decl.get_alias_origin(db, None) {
            return infer_member_by_member_key(db, cache, &origin_type, index_expr, infer_guard);
        } else {
            return infer_member_by_member_key(
                db,
                cache,
                &LuaType::String,
                index_expr,
                infer_guard,
            );
        }
    }

    let owner = LuaMemberOwner::Type(prefix_type_id.clone());
    let key: LuaMemberKey = index_expr.get_index_key()?.into();
    if let Some(member_item) = db.get_member_index().get_member_item(&owner, &key) {
        return member_item.resolve_type(db);
    }

    if type_decl.is_class() {
        let super_types = type_index.get_super_types(&prefix_type_id)?;
        for super_type in super_types {
            if let Some(member_type) =
                infer_member_by_member_key(db, cache, &super_type, index_expr.clone(), infer_guard)
            {
                return Some(member_type);
            }
        }
    }

    None
}

fn infer_tuple_member(tuple_type: &LuaTupleType, index_expr: LuaIndexMemberExpr) -> InferResult {
    let key = index_expr.get_index_key()?.into();
    if let LuaMemberKey::Integer(i) = key {
        let index = if i > 0 { i - 1 } else { 0 };
        return Some(tuple_type.get_type(index as usize)?.clone());
    }

    None
}

fn infer_object_member(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    object_type: &LuaObjectType,
    index_expr: LuaIndexMemberExpr,
) -> InferResult {
    let member_key = index_expr.get_index_key()?;
    if let Some(member_type) = object_type.get_field(&member_key.clone().into()) {
        return Some(member_type.clone());
    }

    let index_accesses = object_type.get_index_access();
    for (key, value) in index_accesses {
        if key.is_string() {
            if member_key.is_string() || member_key.is_name() {
                return Some(value.clone());
            } else if member_key.is_expr() {
                let expr = member_key.get_expr()?;
                let expr_type = infer_expr(db, cache, expr.clone())?;
                if expr_type.is_string() {
                    return Some(value.clone());
                }
            }
        } else if key.is_number() {
            if member_key.is_integer() {
                return Some(value.clone());
            } else if member_key.is_expr() {
                let expr = member_key.get_expr()?;
                let expr_type = infer_expr(db, cache, expr.clone())?;
                if expr_type.is_number() {
                    return Some(value.clone());
                }
            }
        } else if let Some(expr) = member_key.get_expr() {
            let expr_type = infer_expr(db, cache, expr.clone())?;
            if expr_type == *key {
                return Some(value.clone());
            }
        }
    }

    None
}

fn infer_union_member(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    union_type: &LuaUnionType,
    index_expr: LuaIndexMemberExpr,
) -> InferResult {
    let mut member_types = Vec::new();
    for member in union_type.get_types() {
        let member_type = infer_member_by_member_key(
            db,
            cache,
            member,
            index_expr.clone(),
            &mut InferGuard::new(),
        );
        if let Some(member_type) = member_type {
            member_types.push(member_type);
        }
    }

    if member_types.is_empty() {
        return None;
    }

    if member_types.len() == 1 {
        return Some(member_types[0].clone());
    }

    Some(LuaType::Union(LuaUnionType::new(member_types).into()))
}

fn infer_intersection_member(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    intersection_type: &LuaIntersectionType,
    index_expr: LuaIndexMemberExpr,
) -> InferResult {
    let mut member_type = LuaType::Unknown;
    for member in intersection_type.get_types() {
        let sub_member_type = infer_member_by_member_key(
            db,
            cache,
            member,
            index_expr.clone(),
            &mut InferGuard::new(),
        )?;
        if member_type.is_unknown() {
            member_type = sub_member_type;
        } else if member_type != sub_member_type {
            return None;
        }
    }

    Some(member_type)
}

fn infer_generic_member(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    generic_type: &LuaGenericType,
    index_expr: LuaIndexMemberExpr,
) -> InferResult {
    let base_type = generic_type.get_base_type();
    let member_type =
        infer_member_by_member_key(db, cache, &base_type, index_expr, &mut InferGuard::new())?;

    let generic_params = generic_type.get_params();
    let substitutor = TypeSubstitutor::from_type_array(generic_params.clone());
    Some(instantiate_type_generic(db, &member_type, &substitutor))
}

fn infer_instance_member(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    inst: &LuaInstanceType,
    index_expr: LuaIndexMemberExpr,
    infer_guard: &mut InferGuard,
) -> InferResult {
    let range = inst.get_range();

    let origin_type = inst.get_base();
    if let Some(result) =
        infer_member_by_member_key(db, cache, &origin_type, index_expr.clone(), infer_guard)
    {
        return Some(result);
    }

    infer_table_member(db, range.clone(), index_expr.clone())
}

pub fn infer_member_by_operator(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    prefix_type: &LuaType,
    index_expr: LuaIndexMemberExpr,
    infer_guard: &mut InferGuard,
) -> InferResult {
    if without_index_operator(prefix_type) {
        return None;
    }

    match &prefix_type {
        LuaType::TableConst(in_filed) => {
            infer_member_by_index_table(db, cache, in_filed, index_expr)
        }
        LuaType::Ref(decl_id) => {
            infer_member_by_index_custom_type(db, cache, decl_id, index_expr, infer_guard)
        }
        LuaType::Def(decl_id) => {
            infer_member_by_index_custom_type(db, cache, decl_id, index_expr, infer_guard)
        }
        // LuaType::Module(arc) => todo!(),
        LuaType::Array(base) => infer_member_by_index_array(db, cache, base, index_expr),
        LuaType::Object(object) => infer_member_by_index_object(db, cache, object, index_expr),
        LuaType::Union(union) => infer_member_by_index_union(db, cache, union, index_expr),
        LuaType::Intersection(intersection) => {
            infer_member_by_index_intersection(db, cache, intersection, index_expr)
        }
        LuaType::Generic(generic) => infer_member_by_index_generic(db, cache, generic, index_expr),
        LuaType::TableGeneric(table_generic) => {
            infer_member_by_index_table_generic(db, cache, table_generic, index_expr)
        }
        _ => None,
    }
}

fn infer_member_by_index_table(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    table_range: &InFiled<TextRange>,
    index_expr: LuaIndexMemberExpr,
) -> InferResult {
    let syntax_id = LuaSyntaxId::new(LuaSyntaxKind::TableArrayExpr.into(), table_range.value);
    let root = index_expr.get_root();
    let table_array_expr = LuaTableExpr::cast(syntax_id.to_node_from_root(&root)?)?;
    let member_key = index_expr.get_index_key()?;
    match member_key {
        LuaIndexKey::Integer(_) | LuaIndexKey::Expr(_) => {
            let first_field = table_array_expr.get_fields().next()?;
            let first_expr = first_field.get_value_expr()?;
            let ty = infer_expr(db, cache, first_expr)?;
            Some(ty)
        }
        _ => None,
    }
}

fn infer_member_by_index_custom_type(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    prefix_type_id: &LuaTypeDeclId,
    index_expr: LuaIndexMemberExpr,
    infer_guard: &mut InferGuard,
) -> InferResult {
    infer_guard.check(&prefix_type_id)?;
    let type_index = db.get_type_index();
    let type_decl = type_index.get_type_decl(&prefix_type_id)?;
    if type_decl.is_alias() {
        if let Some(origin_type) = type_decl.get_alias_origin(db, None) {
            return infer_member_by_operator(db, cache, &origin_type, index_expr, infer_guard);
        }
        return None;
    }

    let member_key = index_expr.get_index_key()?;
    // find member by key in self
    if let Some(operators_map) = db
        .get_operator_index()
        .get_operators_by_type(prefix_type_id)
    {
        if let Some(index_operator_ids) = operators_map.get(&LuaOperatorMetaMethod::Index) {
            for operator_id in index_operator_ids {
                let operator = db.get_operator_index().get_operator(operator_id)?;
                let operand_type = operator.get_operands().first()?;
                if operand_type.is_string() {
                    if member_key.is_string() || member_key.is_name() {
                        return Some(operator.get_result().clone());
                    } else if member_key.is_expr() {
                        let expr = member_key.get_expr()?;
                        let expr_type = infer_expr(db, cache, expr.clone())?;
                        if expr_type.is_string() {
                            return Some(operator.get_result().clone());
                        }
                    }
                } else if operand_type.is_number() {
                    if member_key.is_integer() {
                        return Some(operator.get_result().clone());
                    } else if member_key.is_expr() {
                        let expr = member_key.get_expr()?;
                        let expr_type = infer_expr(db, cache, expr.clone())?;
                        if expr_type.is_number() {
                            return Some(operator.get_result().clone());
                        }
                    }
                } else if let Some(expr) = member_key.get_expr() {
                    let expr_type = infer_expr(db, cache, expr.clone())?;
                    if expr_type == *operand_type {
                        return Some(operator.get_result().clone());
                    }
                }
            }
        };
    }

    // find member by key in super
    if type_decl.is_class() {
        let super_types = type_index.get_super_types(&prefix_type_id)?;
        for super_type in super_types {
            let member_type =
                infer_member_by_operator(db, cache, &super_type, index_expr.clone(), infer_guard);
            if member_type.is_some() {
                return member_type;
            }
        }
    }

    None
}

fn infer_member_by_index_array(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    base: &LuaType,
    index_expr: LuaIndexMemberExpr,
) -> InferResult {
    let member_key = index_expr.get_index_key()?;
    if member_key.is_integer() {
        return Some(base.clone());
    } else if member_key.is_expr() {
        let expr = member_key.get_expr()?;
        let expr_type = infer_expr(db, cache, expr.clone())?;
        if expr_type.is_number() {
            return Some(base.clone());
        }
    }

    None
}

fn infer_member_by_index_object(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    object: &LuaObjectType,
    index_expr: LuaIndexMemberExpr,
) -> InferResult {
    let member_key = index_expr.get_index_key()?;
    let access_member_type = object.get_index_access();
    if member_key.is_expr() {
        let expr = member_key.get_expr()?;
        let expr_type = infer_expr(db, cache, expr.clone())?;
        for (key, field) in access_member_type {
            if *key == expr_type {
                return Some(field.clone());
            }
        }
    }

    None
}

fn infer_member_by_index_union(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    union: &LuaUnionType,
    index_expr: LuaIndexMemberExpr,
) -> InferResult {
    let mut member_types = Vec::new();
    for member in union.get_types() {
        let member_type = infer_member_by_operator(
            db,
            cache,
            member,
            index_expr.clone(),
            &mut InferGuard::new(),
        );
        if let Some(member_type) = member_type {
            member_types.push(member_type);
        }
    }

    if member_types.is_empty() {
        return None;
    }

    if member_types.len() == 1 {
        return Some(member_types[0].clone());
    }

    Some(LuaType::Union(LuaUnionType::new(member_types).into()))
}

fn infer_member_by_index_intersection(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    intersection: &LuaIntersectionType,
    index_expr: LuaIndexMemberExpr,
) -> InferResult {
    let mut member_type = LuaType::Unknown;
    for member in intersection.get_types() {
        let sub_member_type = infer_member_by_operator(
            db,
            cache,
            member,
            index_expr.clone(),
            &mut InferGuard::new(),
        )?;
        if member_type.is_unknown() {
            member_type = sub_member_type;
        } else if member_type != sub_member_type {
            return None;
        }
    }

    Some(member_type)
}

fn infer_member_by_index_generic(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    generic: &LuaGenericType,
    index_expr: LuaIndexMemberExpr,
) -> InferResult {
    let base_type = generic.get_base_type();
    let type_decl_id = if let LuaType::Ref(id) = base_type {
        id
    } else {
        return None;
    };
    let generic_params = generic.get_params();
    let substitutor = TypeSubstitutor::from_type_array(generic_params.clone());
    let type_index = db.get_type_index();
    let type_decl = type_index.get_type_decl(&type_decl_id)?;
    if type_decl.is_alias() {
        if let Some(origin_type) = type_decl.get_alias_origin(db, Some(&substitutor)) {
            return infer_member_by_operator(
                db,
                cache,
                &instantiate_type_generic(db, &origin_type, &substitutor),
                index_expr.clone(),
                &mut InferGuard::new(),
            );
        }
        return None;
    }

    let member_key = index_expr.get_index_key()?;
    let operator_index = db.get_operator_index();
    if let Some(operator_maps) = operator_index.get_operators_by_type(&type_decl_id) {
        let index_operator_ids = operator_maps.get(&LuaOperatorMetaMethod::Index)?;
        for index_operator_id in index_operator_ids {
            let index_operator = operator_index.get_operator(index_operator_id)?;
            let operand_type = index_operator.get_operands().first()?;
            let instianted_operand_type = instantiate_type_generic(db, &operand_type, &substitutor);
            if instianted_operand_type.is_string() {
                if member_key.is_string() || member_key.is_name() {
                    return Some(instantiate_type_generic(
                        db,
                        index_operator.get_result(),
                        &substitutor,
                    ));
                } else if member_key.is_expr() {
                    let expr = member_key.get_expr()?;
                    let expr_type = infer_expr(db, cache, expr.clone())?;
                    if expr_type.is_string() {
                        return Some(instantiate_type_generic(
                            db,
                            index_operator.get_result(),
                            &substitutor,
                        ));
                    }
                }
            } else if instianted_operand_type.is_number() {
                if member_key.is_integer() {
                    return Some(instantiate_type_generic(
                        db,
                        index_operator.get_result(),
                        &substitutor,
                    ));
                } else if member_key.is_expr() {
                    let expr = member_key.get_expr()?;
                    let expr_type = infer_expr(db, cache, expr.clone())?;
                    if expr_type.is_number() {
                        return Some(instantiate_type_generic(
                            db,
                            index_operator.get_result(),
                            &substitutor,
                        ));
                    }
                }
            } else if let Some(expr) = member_key.get_expr() {
                let expr_type = infer_expr(db, cache, expr.clone())?;
                if expr_type == *operand_type {
                    return Some(instantiate_type_generic(
                        db,
                        index_operator.get_result(),
                        &substitutor,
                    ));
                }
            }
        }
    }

    // for supers
    let supers = type_index.get_super_types(&type_decl_id)?;
    for super_type in supers {
        let member_type = infer_member_by_operator(
            db,
            cache,
            &instantiate_type_generic(db, &super_type, &substitutor),
            index_expr.clone(),
            &mut InferGuard::new(),
        );
        if member_type.is_some() {
            return member_type;
        }
    }

    None
}

fn infer_member_by_index_table_generic(
    db: &DbIndex,
    cache: &mut LuaInferCache,
    table_params: &Vec<LuaType>,
    index_expr: LuaIndexMemberExpr,
) -> InferResult {
    if table_params.len() != 2 {
        return None;
    }

    let member_key = index_expr.get_index_key()?;
    let key_type = &table_params[0];
    let value_type = &table_params[1];
    if key_type.is_string() {
        if member_key.is_string() || member_key.is_name() {
            return Some(value_type.clone());
        } else if member_key.is_expr() {
            let expr = member_key.get_expr()?;
            let expr_type = infer_expr(db, cache, expr.clone())?;
            if expr_type.is_string() {
                return Some(value_type.clone());
            }
        }
    } else if key_type.is_number() {
        if member_key.is_integer() {
            return Some(value_type.clone());
        } else if member_key.is_expr() {
            let expr = member_key.get_expr()?;
            let expr_type = infer_expr(db, cache, expr.clone())?;
            if expr_type.is_number() {
                return Some(value_type.clone());
            }
        }
    } else {
        let expr_type = match member_key {
            LuaIndexKey::Expr(expr) => infer_expr(db, cache, expr.clone())?,
            LuaIndexKey::Integer(i) => LuaType::IntegerConst(i.get_int_value()),
            LuaIndexKey::String(s) => LuaType::StringConst(SmolStr::new(&s.get_value()).into()),
            _ => return None,
        };

        if check_type_compact(db, key_type, &expr_type).is_ok() {
            return Some(value_type.clone());
        }
    }

    None
}

fn infer_global_field_member(
    db: &DbIndex,
    _: &LuaInferCache,
    index_expr: LuaIndexMemberExpr,
) -> InferResult {
    let member_key = index_expr.get_index_key()?;
    let name = member_key.get_name()?.get_name_text();
    let global_member = db
        .get_decl_index()
        .get_global_decl_id(&LuaMemberKey::Name(name.to_string().into()))?;

    let decl = db.get_decl_index().get_decl(&global_member)?;
    Some(decl.get_type()?.clone())
}

fn infer_namespace_member(
    db: &DbIndex,
    _: &LuaInferCache,
    ns: &str,
    index_expr: LuaIndexMemberExpr,
) -> InferResult {
    let member_key = index_expr.get_index_key()?;
    let member_key = match member_key.into() {
        LuaMemberKey::Name(name) => name,
        _ => return None,
    };

    let namespace_or_type_id = format!("{}.{}", ns, member_key);
    let type_id = LuaTypeDeclId::new(&namespace_or_type_id);
    if db.get_type_index().get_type_decl(&type_id).is_some() {
        return Some(LuaType::Def(type_id));
    }

    return Some(LuaType::Namespace(
        SmolStr::new(namespace_or_type_id).into(),
    ));
}
