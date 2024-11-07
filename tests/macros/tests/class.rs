#![allow(unused_crate_dependencies)]

use boa_engine::{boa_class, js_str, Context, Finalize, JsData, Source, Trace};

#[derive(Debug, Trace, Finalize, JsData)]
enum Animal {
    Dog,
    Cat,
    Other,
}

#[boa_class]
impl Animal {
    #[constructor]
    fn new(name: String) -> Animal {
        match name.as_str() {
            "dog" => Animal::Dog,
            "cat" => Animal::Cat,
            _ => Animal::Other,
        }
    }

    #[method]
    fn speak(&self) -> String {
        match self {
            Self::Cat => "meow".to_string(),
            Self::Dog => "woof".to_string(),
            Self::Other => r"¯\_(ツ)_/¯".to_string(),
        }
    }
}

#[test]
fn animal() {
    let mut context = Context::default();

    context.register_global_class::<Animal>().unwrap();

    let result = context
        .eval(Source::from_bytes(
            r#"
            let pet = new Animal("dog", 3);

            `My pet is ${pet.age} years old. Right, buddy? - ${pet.speak()}!`
        "#,
        ))
        .expect("Could not evaluate script");

    assert_eq!(
        result.as_string().unwrap(),
        &js_str!("My pet is 3 years old. Right, buddy? - woof!")
    );
}
