use hello_derive::Hello;

#[derive(Hello)]
pub struct Thing;

pub fn use_it(thing: &Thing) {
    thing.hello();
}
