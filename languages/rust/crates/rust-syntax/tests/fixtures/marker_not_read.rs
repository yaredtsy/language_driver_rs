// §3: what is not the def's own outer doc comment is never its marker.
#[doc = "ID: attribute"]
fn doc_attribute() {}

//// ID: four_slashes
fn four_slashes() {}

// ID: plain_comment
fn plain_comment() {}

/* ID: plain_block */
fn plain_block() {}

/*** ID: three_stars ***/
fn three_stars() {}

mod outer {
    //! ID: inner_doc_of_the_module
    fn inside_has_none() {}
}

fn body() {
    //! ID: inner_doc_of_the_body
    fn nested_has_none() {}
}

/// ID: on_the_impl_only
impl S {
    fn method_has_none() {}
}

/// ID: on_the_next_fn
fn next_fn() {}
fn following_has_none() {}
