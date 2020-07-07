use core::convert::{TryFrom, TryInto};
use core::pin::Pin;
use sixtyfps_compilerlib::expression_tree::{
    Expression, NamedReference, PathElement as ExprPathElement,
};
use sixtyfps_compilerlib::{object_tree::ElementRc, typeregister::Type};
use sixtyfps_corelib as corelib;
use sixtyfps_corelib::{
    abi::datastructures::ItemRef, abi::primitives::PropertyAnimation, Color, EvaluationContext,
    PathElements, Resource, SharedString,
};
use std::{collections::HashMap, rc::Rc};

pub trait ErasedPropertyInfo {
    fn get(&self, item: Pin<ItemRef>, context: &EvaluationContext) -> Value;
    fn set(&self, item: Pin<ItemRef>, value: Value, animation: Option<PropertyAnimation>);
    fn set_binding(
        &self,
        item: Pin<ItemRef>,
        binding: Box<dyn Fn(&EvaluationContext) -> Value>,
        animation: Option<PropertyAnimation>,
    );
    fn offset(&self) -> usize;
}

impl<Item: vtable::HasStaticVTable<corelib::abi::datastructures::ItemVTable>> ErasedPropertyInfo
    for &'static dyn corelib::rtti::PropertyInfo<Item, Value>
{
    fn get(&self, item: Pin<ItemRef>, context: &EvaluationContext) -> Value {
        (*self).get(ItemRef::downcast_pin(item).unwrap(), context).unwrap()
    }
    fn set(&self, item: Pin<ItemRef>, value: Value, animation: Option<PropertyAnimation>) {
        (*self).set(ItemRef::downcast_pin(item).unwrap(), value, animation).unwrap()
    }
    fn set_binding(
        &self,
        item: Pin<ItemRef>,
        binding: Box<dyn Fn(&EvaluationContext) -> Value>,
        animation: Option<PropertyAnimation>,
    ) {
        (*self).set_binding(ItemRef::downcast_pin(item).unwrap(), binding, animation).unwrap();
    }
    fn offset(&self) -> usize {
        (*self).offset()
    }
}

#[derive(Debug, Clone, PartialEq)]
/// This is a dynamically typed Value used in the interpreter, it need to be able
/// to be converted from and to anything that can be stored in a Property
pub enum Value {
    /// There is nothing in this value. That's the default.
    /// For example, a function that do not return a result would return a Value::Void
    Void,
    /// An i32 or a float
    Number(f64),
    /// String
    String(SharedString),
    /// Bool
    Bool(bool),
    /// A resource (typically an image)
    Resource(Resource),
    /// An Array
    Array(Vec<Value>),
    /// An object
    Object(HashMap<String, Value>),
    /// A color
    Color(Color),
    /// The elements of a path
    PathElements(PathElements),
}

impl Default for Value {
    fn default() -> Self {
        Value::Void
    }
}

impl corelib::rtti::ValueType for Value {}

/// Helper macro to implement the TryFrom / TryInto for Value
///
/// For example
/// `declare_value_conversion!(Number => [u32, u64, i32, i64, f32, f64] );`
/// means that Value::Number can be converted to / from each of the said rust types
macro_rules! declare_value_conversion {
    ( $value:ident => [$($ty:ty),*] ) => {
        $(
            impl TryFrom<$ty> for Value {
                type Error = ();
                fn try_from(v: $ty) -> Result<Self, ()> {
                    //Ok(Value::$value(v.try_into().map_err(|_|())?))
                    Ok(Value::$value(v as _))
                }
            }
            impl TryInto<$ty> for Value {
                type Error = ();
                fn try_into(self) -> Result<$ty, ()> {
                    match self {
                        //Self::$value(x) => x.try_into().map_err(|_|()),
                        Self::$value(x) => Ok(x as _),
                        _ => Err(())
                    }
                }
            }
        )*
    };
}
declare_value_conversion!(Number => [u32, u64, i32, i64, f32, f64, usize, isize] );
declare_value_conversion!(String => [SharedString] );
declare_value_conversion!(Bool => [bool] );
declare_value_conversion!(Resource => [Resource] );
declare_value_conversion!(Object => [HashMap<String, Value>] );
declare_value_conversion!(Color => [Color] );
declare_value_conversion!(PathElements => [PathElements]);

/// Evaluate an expression and return a Value as the result of this expression
pub fn eval_expression(
    e: &Expression,
    component_type: &crate::ComponentDescription,
    eval_context: &corelib::EvaluationContext,
) -> Value {
    match e {
        Expression::Invalid => panic!("invalid expression while evaluating"),
        Expression::Uncompiled(_) => panic!("uncompiled expression while evaluating"),
        Expression::StringLiteral(s) => Value::String(s.as_str().into()),
        Expression::NumberLiteral(n) => Value::Number(*n),
        Expression::BoolLiteral(b) => Value::Bool(*b),
        Expression::SignalReference { .. } => panic!("signal in expression"),
        Expression::PropertyReference(NamedReference { element, name }) => {
            let element = element.upgrade().unwrap();
            let (component_mem, component_type, eval_context) =
                enclosing_component_for_element(&element, eval_context);
            let element = element.borrow();
            if element.id == element.enclosing_component.upgrade().unwrap().root_element.borrow().id
            {
                if let Some(x) = component_type.custom_properties.get(name) {
                    return unsafe {
                        x.prop
                            .get(Pin::new_unchecked(&*component_mem.add(x.offset)), &eval_context)
                            .unwrap()
                    };
                }
            };
            let item_info = &component_type.items[element.id.as_str()];
            core::mem::drop(element);
            let item = unsafe { item_info.item_from_component(component_mem) };
            item_info.rtti.properties[name.as_str()].get(item, &eval_context)
        }
        Expression::RepeaterIndexReference { element } => {
            if element.upgrade().unwrap().borrow().base_type
                == Type::Component(component_type.original.clone())
            {
                let x = &component_type.custom_properties["index"];
                unsafe {
                    x.prop
                        .get(
                            Pin::new_unchecked(&*eval_context.component.as_ptr().add(x.offset)),
                            &eval_context,
                        )
                        .unwrap()
                }
            } else {
                todo!();
            }
        }
        Expression::RepeaterModelReference { element } => {
            if element.upgrade().unwrap().borrow().base_type
                == Type::Component(component_type.original.clone())
            {
                let x = &component_type.custom_properties["model_data"];
                unsafe {
                    x.prop
                        .get(
                            Pin::new_unchecked(&*eval_context.component.as_ptr().add(x.offset)),
                            &eval_context,
                        )
                        .unwrap()
                }
            } else {
                todo!();
            }
        }
        Expression::ObjectAccess { base, name } => {
            if let Value::Object(mut o) = eval_expression(base, component_type, eval_context) {
                o.remove(name).unwrap_or(Value::Void)
            } else {
                Value::Void
            }
        }
        Expression::Cast { from, to } => {
            let v = eval_expression(&*from, component_type, eval_context);
            match (v, to) {
                (Value::Number(n), Type::Int32) => Value::Number(n.round()),
                (Value::Number(n), Type::String) => {
                    Value::String(SharedString::from(format!("{}", n).as_str()))
                }
                (Value::Number(n), Type::Color) => Value::Color(Color::from(n as u32)),
                (v, _) => v,
            }
        }
        Expression::CodeBlock(sub) => {
            let mut v = Value::Void;
            for e in sub {
                v = eval_expression(e, component_type, eval_context);
            }
            v
        }
        Expression::FunctionCall { function, .. } => {
            if let Expression::SignalReference(NamedReference { element, name }) = &**function {
                let element = element.upgrade().unwrap();
                let (component_mem, component_type, eval_context) =
                    enclosing_component_for_element(&element, eval_context);

                let item_info = &component_type.items[element.borrow().id.as_str()];
                let item = unsafe { item_info.item_from_component(component_mem) };
                let signal = unsafe {
                    &mut *(item_info
                        .rtti
                        .signals
                        .get(name.as_str())
                        .map(|o| item.as_ptr().add(*o) as *const u8)
                        .or_else(|| {
                            component_type
                                .custom_signals
                                .get(name.as_str())
                                .map(|o| component_mem.add(*o))
                        })
                        .unwrap_or_else(|| panic!("unkown signal {}", name))
                        as *mut corelib::Signal<()>)
                };
                signal.emit(eval_context, ());
                Value::Void
            } else {
                panic!("call of something not a signal")
            }
        }
        Expression::SelfAssignment { lhs, rhs, op } => match &**lhs {
            Expression::PropertyReference(NamedReference { element, name }) => {
                let eval = |lhs| {
                    let rhs = eval_expression(&**rhs, component_type, eval_context);
                    match (lhs, rhs, op) {
                        (Value::Number(a), Value::Number(b), '+') => Value::Number(a + b),
                        (Value::Number(a), Value::Number(b), '-') => Value::Number(a - b),
                        (Value::Number(a), Value::Number(b), '/') => Value::Number(a / b),
                        (Value::Number(a), Value::Number(b), '*') => Value::Number(a * b),
                        (lhs, rhs, op) => panic!("unsupported {:?} {} {:?}", lhs, op, rhs),
                    }
                };

                let element = element.upgrade().unwrap();
                let (component_mem, component_type, eval_context) =
                    enclosing_component_for_element(&element, eval_context);

                let component = element.borrow().enclosing_component.upgrade().unwrap();
                if element.borrow().id == component.root_element.borrow().id {
                    if let Some(x) = component_type.custom_properties.get(name) {
                        unsafe {
                            let p = Pin::new_unchecked(&*component_mem.add(x.offset));
                            x.prop
                                .set(p, eval(x.prop.get(p, &eval_context).unwrap()), None)
                                .unwrap();
                        }
                        return Value::Void;
                    }
                };
                let item_info = &component_type.items[element.borrow().id.as_str()];
                let item = unsafe { item_info.item_from_component(component_mem) };
                let p = &item_info.rtti.properties[name.as_str()];
                p.set(item, eval(p.get(item, &eval_context)), None);
                Value::Void
            }
            _ => panic!("typechecking should make sure this was a PropertyReference"),
        },
        Expression::BinaryExpression { lhs, rhs, op } => {
            let lhs = eval_expression(&**lhs, component_type, eval_context);
            let rhs = eval_expression(&**rhs, component_type, eval_context);

            match (op, lhs, rhs) {
                ('+', Value::Number(a), Value::Number(b)) => Value::Number(a + b),
                ('-', Value::Number(a), Value::Number(b)) => Value::Number(a - b),
                ('/', Value::Number(a), Value::Number(b)) => Value::Number(a / b),
                ('*', Value::Number(a), Value::Number(b)) => Value::Number(a * b),
                ('<', Value::Number(a), Value::Number(b)) => Value::Bool(a < b),
                ('>', Value::Number(a), Value::Number(b)) => Value::Bool(a > b),
                ('≤', Value::Number(a), Value::Number(b)) => Value::Bool(a <= b),
                ('≥', Value::Number(a), Value::Number(b)) => Value::Bool(a >= b),
                ('=', a, b) => Value::Bool(a == b),
                ('!', a, b) => Value::Bool(a != b),
                ('&', Value::Bool(a), Value::Bool(b)) => Value::Bool(a && b),
                ('|', Value::Bool(a), Value::Bool(b)) => Value::Bool(a || b),
                (op, lhs, rhs) => panic!("unsupported {:?} {} {:?}", lhs, op, rhs),
            }
        }
        Expression::UnaryOp { sub, op } => {
            let sub = eval_expression(&**sub, component_type, eval_context);
            match (sub, op) {
                (Value::Number(a), '+') => Value::Number(a),
                (Value::Number(a), '-') => Value::Number(-a),
                (Value::Bool(a), '!') => Value::Bool(!a),
                (sub, op) => panic!("unsupported {} {:?}", op, sub),
            }
        }
        Expression::ResourceReference { absolute_source_path } => {
            Value::Resource(Resource::AbsoluteFilePath(absolute_source_path.as_str().into()))
        }
        Expression::Condition { condition, true_expr, false_expr } => {
            match eval_expression(&**condition, component_type, eval_context).try_into()
                as Result<bool, _>
            {
                Ok(true) => eval_expression(&**true_expr, component_type, eval_context),
                Ok(false) => eval_expression(&**false_expr, component_type, eval_context),
                _ => panic!("conditional expression did not evaluate to boolean"),
            }
        }
        Expression::Array { values, .. } => Value::Array(
            values.iter().map(|e| eval_expression(e, component_type, eval_context)).collect(),
        ),
        Expression::Object { values, .. } => Value::Object(
            values
                .iter()
                .map(|(k, v)| (k.clone(), eval_expression(v, component_type, eval_context)))
                .collect(),
        ),
        Expression::PathElements { elements } => {
            Value::PathElements(PathElements::SharedElements(sixtyfps_corelib::SharedArray::<
                sixtyfps_corelib::abi::datastructures::PathElement,
            >::from_iter(
                elements
                    .iter()
                    .map(|element| convert_path_element(element, component_type, eval_context)),
            )))
        }
    }
}

fn enclosing_component_for_element<'a>(
    element: &ElementRc,
    eval_context: &'a corelib::EvaluationContext<'a>,
) -> (*const u8, &'a crate::ComponentDescription, &'a corelib::EvaluationContext<'a>) {
    let component_type =
        unsafe { crate::dynamic_component::get_component_type(eval_context.component) };
    if Rc::ptr_eq(
        &element.borrow().enclosing_component.upgrade().unwrap(),
        &component_type.original,
    ) {
        let mem = eval_context.component.as_ptr();
        (mem, component_type, eval_context)
    } else {
        debug_assert!(component_type.original.parent_element.upgrade().is_some());
        enclosing_component_for_element(element, eval_context.parent_context.unwrap())
    }
}

pub fn new_struct_with_bindings<
    ElementType: 'static + Default + sixtyfps_corelib::rtti::BuiltinItem,
>(
    bindings: &HashMap<String, Expression>,
    component_type: &crate::ComponentDescription,
    eval_context: &corelib::EvaluationContext,
) -> ElementType {
    let mut element = ElementType::default();
    for (prop, info) in ElementType::fields::<Value>().into_iter() {
        if let Some(binding) = &bindings.get(prop) {
            let value = eval_expression(&binding, &*component_type, &eval_context);
            info.set_field(&mut element, value).unwrap();
        }
    }
    element
}

fn convert_path_element(
    expr_element: &ExprPathElement,
    component_type: &crate::ComponentDescription,
    eval_context: &corelib::EvaluationContext,
) -> sixtyfps_corelib::abi::datastructures::PathElement {
    match expr_element.element_type.class_name.as_str() {
        "LineTo" => sixtyfps_corelib::abi::datastructures::PathElement::LineTo(
            new_struct_with_bindings(&expr_element.bindings, component_type, eval_context),
        ),
        "ArcTo" => sixtyfps_corelib::abi::datastructures::PathElement::ArcTo(
            new_struct_with_bindings(&expr_element.bindings, component_type, eval_context),
        ),
        _ => panic!(
            "Cannot create unsupported path element {}",
            expr_element.element_type.class_name
        ),
    }
}
