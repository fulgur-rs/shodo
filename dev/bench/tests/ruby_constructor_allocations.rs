#![cfg(feature = "allocation-counting")]

use shodo::ParagraphBuilder;
use shodo::limits::{LimitKind, Limits};
use shodo::node::NodeId;
use shodo::ruby::{
    Ruby, RubyAnnotation, RubyContent, RubyError, RubyLevel, RubySpan, RubyStyle, RubyVisibility,
};
use shodo::style::ParagraphStyle;
use shodo_bench::allocator::CountingAllocator;

#[global_allocator]
static ALLOC: CountingAllocator<std::alloc::System> = CountingAllocator::new(std::alloc::System);

#[test]
fn rejected_constructor_does_not_allocate_a_span_table() {
    let content = RubyContent::from_builder(ParagraphBuilder::new(
        &ParagraphStyle::default(),
        &Limits::default(),
    ));
    for visibility in [
        RubyVisibility::Visible,
        RubyVisibility::Hidden,
        RubyVisibility::Collapse,
    ] {
        let levels = vec![RubyLevel {
            annotations: vec![
                RubyAnnotation {
                    node: NodeId(1),
                    content: content.clone(),
                    span: RubySpan::Auto,
                    visibility,
                };
                1024
            ],
            style: RubyStyle::default(),
        }];
        let limits = Limits {
            max_items: Some(1025),
            ..Default::default()
        };
        // Caller-owned input and content are constructed outside measurement.
        // The shared snapshot also survives rejection outside the scope.
        let scope = ALLOC.begin().unwrap();
        let result = Ruby::with_limits(vec![], levels, &limits);
        let counts = scope.finish();
        let Err(error) = result else {
            panic!("over-budget ruby must be rejected");
        };
        let RubyError::Limit(error) = error else {
            panic!("expected resource error")
        };
        assert_eq!(
            (error.kind, error.limit, error.actual),
            (LimitKind::Items, 1025, 1026)
        );
        assert_eq!(
            counts.calls, 0,
            "rejected constructor allocated: {counts:?}"
        );
        assert_eq!(counts.allocated_bytes, 0);
    }
}
