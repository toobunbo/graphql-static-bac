pub fn type_id(type_name: &str) -> String {
    format!("type:{type_name}")
}

pub fn field_id(owner: &str, field: &str) -> String {
    format!("field:{owner}.{field}")
}

pub fn arg_id(owner: &str, field: &str, argument: &str) -> String {
    format!("arg:{owner}.{field}.{argument}")
}

pub fn input_field_id(owner: &str, field: &str) -> String {
    format!("input_field:{owner}.{field}")
}
