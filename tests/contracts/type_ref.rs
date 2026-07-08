use graphql_static_bac::contracts::{
    arg_id, field_id, input_field_id, type_id, TypeKind, TypeRef, TypeRefError, TypeWrapper,
};

#[test]
fn renders_canonical_type_refs() {
    let plain = TypeRef::new("User", TypeKind::Object, vec![]).unwrap();
    assert_eq!(plain.display, "User");

    let non_null = TypeRef::new("User", TypeKind::Object, vec![TypeWrapper::NonNull]).unwrap();
    assert_eq!(non_null.display, "User!");

    let nested = TypeRef::new(
        "User",
        TypeKind::Object,
        vec![
            TypeWrapper::NonNull,
            TypeWrapper::List,
            TypeWrapper::NonNull,
        ],
    )
    .unwrap();
    assert_eq!(nested.display, "[User!]!");
}

#[test]
fn rejects_double_non_null() {
    let result = TypeRef::new(
        "User",
        TypeKind::Object,
        vec![TypeWrapper::NonNull, TypeWrapper::NonNull],
    );
    assert_eq!(result, Err(TypeRefError::DoubleNonNull));
}

#[test]
fn builds_stable_ids() {
    assert_eq!(type_id("User"), "type:User");
    assert_eq!(field_id("Query", "user"), "field:Query.user");
    assert_eq!(arg_id("Query", "user", "id"), "arg:Query.user.id");
    assert_eq!(
        input_field_id("SearchFilter", "ownerId"),
        "input_field:SearchFilter.ownerId"
    );
}
