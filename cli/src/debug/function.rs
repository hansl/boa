use boa_engine::{
    Context, JsArgs, JsNativeError, JsObject, JsResult, JsValue, NativeFunction,
    builtins::function::OrdinaryFunction,
    js_string,
    object::ObjectInitializer,
    vm::flowgraph::{Direction, Graph},
};
use cow_utils::CowUtils;

use crate::FlowgraphFormat;

fn flowgraph_parse_format_option(value: &JsValue) -> JsResult<FlowgraphFormat> {
    if value.is_undefined() {
        return Ok(FlowgraphFormat::Mermaid);
    }
    if let Some(string) = value.as_string() {
        return match string.to_std_string_escaped().cow_to_lowercase().as_ref() {
            "mermaid" => Ok(FlowgraphFormat::Mermaid),
            "graphviz" => Ok(FlowgraphFormat::Graphviz),
            format => Err(JsNativeError::typ()
                .with_message(format!("Unknown format type '{format}'"))
                .into()),
        };
    }
    Err(JsNativeError::typ()
        .with_message("format type must be a string")
        .into())
}

fn flowgraph_parse_direction_option(value: &JsValue) -> JsResult<Direction> {
    if value.is_undefined() {
        return Ok(Direction::LeftToRight);
    }
    if let Some(string) = value.as_string() {
        return match string.to_std_string_escaped().cow_to_lowercase().as_ref() {
            "leftright" | "lr" => Ok(Direction::LeftToRight),
            "rightleft" | "rl" => Ok(Direction::RightToLeft),
            "topbottom" | "tb" => Ok(Direction::TopToBottom),
            "bottomtop" | "bt " => Ok(Direction::BottomToTop),
            direction => Err(JsNativeError::typ()
                .with_message(format!("Unknown direction type '{direction}'"))
                .into()),
        };
    }
    Err(JsNativeError::typ()
        .with_message("direction type must be a string")
        .into())
}

/// Get functions instruction flowgraph
fn flowgraph(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let Some(value) = args.first() else {
        return Err(JsNativeError::typ()
            .with_message("expected function argument")
            .into());
    };
    let Some(object) = value.as_object() else {
        return Err(JsNativeError::typ()
            .with_message(format!("expected object, got {}", value.type_of()))
            .into());
    };
    let mut format = FlowgraphFormat::Mermaid;
    let mut direction = Direction::LeftToRight;
    if let Some(arguments) = args.get(1) {
        if let Some(arguments) = arguments.as_object() {
            format = flowgraph_parse_format_option(&arguments.get(js_string!("format"), context)?)?;
            direction = flowgraph_parse_direction_option(
                &arguments.get(js_string!("direction"), context)?,
            )?;
        } else {
            format = flowgraph_parse_format_option(arguments)?;
        }
    }
    let Some(function) = object.downcast_ref::<OrdinaryFunction>() else {
        return Err(JsNativeError::typ()
            .with_message("expected an ordinary function object")
            .into());
    };
    let code = function.codeblock();
    let mut graph = Graph::new(direction);
    code.to_graph(graph.subgraph(String::default()));
    let result = match format {
        FlowgraphFormat::Graphviz => graph.to_graphviz_format(),
        FlowgraphFormat::Mermaid => graph.to_mermaid_format(),
    };
    Ok(JsValue::new(js_string!(result)))
}

fn bytecode(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let Some(value) = args.first() else {
        return Err(JsNativeError::typ()
            .with_message("expected function argument")
            .into());
    };

    let Some(object) = value.as_object() else {
        return Err(JsNativeError::typ()
            .with_message(format!("expected object, got {}", value.type_of()))
            .into());
    };
    let Some(function) = object.downcast_ref::<OrdinaryFunction>() else {
        return Err(JsNativeError::typ()
            .with_message("expected an ordinary function object")
            .into());
    };
    let code = function.codeblock();

    println!("{code}");

    Ok(JsValue::undefined())
}

fn set_trace_flag_in_function_object(object: &JsObject, value: bool) -> JsResult<()> {
    let Some(function) = object.downcast_ref::<OrdinaryFunction>() else {
        return Err(JsNativeError::typ()
            .with_message("expected an ordinary function object")
            .into());
    };
    let code = function.codeblock();
    code.set_traceable(value);
    Ok(())
}

/// Trace function.
fn trace(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args.get_or_undefined(0);
    let this = args.get_or_undefined(1);

    let Some(callable) = value.as_callable() else {
        return Err(JsNativeError::typ()
            .with_message("expected callable object")
            .into());
    };

    let arguments = args.get(2..).unwrap_or(&[]);

    set_trace_flag_in_function_object(&callable, true)?;
    let result = callable.call(this, arguments, context);
    set_trace_flag_in_function_object(&callable, false)?;

    result
}

fn traceable(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let value = args.get_or_undefined(0);
    let traceable = args.get_or_undefined(1).to_boolean();

    let Some(callable) = value.as_callable() else {
        return Err(JsNativeError::typ()
            .with_message("expected callable object")
            .into());
    };

    set_trace_flag_in_function_object(&callable, traceable)?;

    Ok(value.clone())
}

pub(super) fn create_object(context: &mut Context) -> JsObject {
    ObjectInitializer::new(context)
        .function(
            NativeFunction::from_fn_ptr(flowgraph),
            js_string!("flowgraph"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(bytecode),
            js_string!("bytecode"),
            1,
        )
        .function(NativeFunction::from_fn_ptr(trace), js_string!("trace"), 1)
        .function(
            NativeFunction::from_fn_ptr(traceable),
            js_string!("traceable"),
            2,
        )
        .build()
}
