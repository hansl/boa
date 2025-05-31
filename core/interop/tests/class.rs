use boa_engine::{js_str, js_string, Context, JsString, Source};
use boa_macros::{class as boa_class, Finalize, JsData, Trace};

#[derive(Clone, Trace, Finalize, JsData)]
enum AnimalType {
    Cat,
    Dog,
    Other,
}

#[derive(Clone, Trace, Finalize, JsData)]
struct Animal {
    ty: AnimalType,
    age: i32,
}

#[boa_class]
impl Animal {
    #[boa(constructor)]
    fn new(name: String, age: i32) -> Self {
        let ty = match name.as_str() {
            "cat" => AnimalType::Cat,
            "dog" => AnimalType::Dog,
            _ => AnimalType::Other,
        };

        Self { ty, age }
    }

    #[boa(getter)]
    fn age(&self) -> i32 {
        self.age
    }

    fn speak(&self) -> JsString {
        match self.ty {
            AnimalType::Cat => js_string!("meow"),
            AnimalType::Dog => js_string!("woof"),
            AnimalType::Other => js_string!(r"¯\_(ツ)_/¯"),
        }
    }
}

fn main() {
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
