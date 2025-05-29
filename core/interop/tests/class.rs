#[derive(Clone, Trace, Finalize, JsData)]
enum Animal {
    Cat,
    Dog,
    Other,
}

#[boa::class]
impl Animal {
    #[boa(constructor)]
    fn ctor(name: String, age: i32) -> Result<Self> {
        match name.as_str() {
            "cat" => Ok(Animal::Cat),
            "dog" => Ok(Animal::Dog),
            _ => Ok(Animal::Other),
        }
    }

    #[boa(object, getter)]
    fn age(this: JsObject, _: Ignore, age: i32) -> i32 {
        age
    }

    fn speak(&self) -> JsString {
        match self {
            Animal::Cat => js_string!("meow"),
            Animal::Dog => js_string!("woof"),
            Animal::Other => js_string!(r"¯\_(ツ)_/¯"),
        }
    }
}
