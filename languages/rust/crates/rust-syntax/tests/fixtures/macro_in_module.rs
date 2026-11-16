// D9: a macro_rules! is named by its module path plus m!, like every other item, not the way it is called. #[macro_export] does not move it.
macro_rules! top { () => {}; }
mod outer {
    macro_rules! shallow { () => {}; }
    mod inner {
        macro_rules! deep { () => {}; }
        #[macro_export]
        macro_rules! exported { () => {}; }
    }
}
