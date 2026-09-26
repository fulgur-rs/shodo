use shodo::geometry::{
    Direction, LogicalRect, PhysicalConverter, PhysicalRect, PhysicalSize, WritingMode,
};

const CONTAINER: PhysicalSize = PhysicalSize {
    width: 200.0,
    height: 100.0,
};
const R: LogicalRect = LogicalRect {
    inline_start: 10.0,
    block_start: 20.0,
    inline_size: 30.0,
    block_size: 40.0,
};

fn convert(wm: WritingMode, dir: Direction) -> PhysicalRect {
    PhysicalConverter::new(wm, dir, CONTAINER).rect(R)
}

#[test]
fn horizontal_tb() {
    assert_eq!(
        convert(WritingMode::HorizontalTb, Direction::Ltr),
        PhysicalRect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0
        }
    );
    // RTL: inline-start is the right edge.
    assert_eq!(
        convert(WritingMode::HorizontalTb, Direction::Rtl),
        PhysicalRect {
            x: 160.0,
            y: 20.0,
            width: 30.0,
            height: 40.0
        }
    );
}

#[test]
fn vertical_rl_blocks_flow_right_to_left() {
    assert_eq!(
        convert(WritingMode::VerticalRl, Direction::Ltr),
        PhysicalRect {
            x: 140.0,
            y: 10.0,
            width: 40.0,
            height: 30.0
        }
    );
    assert_eq!(
        convert(WritingMode::VerticalRl, Direction::Rtl),
        PhysicalRect {
            x: 140.0,
            y: 60.0,
            width: 40.0,
            height: 30.0
        }
    );
    assert_eq!(
        convert(WritingMode::SidewaysRl, Direction::Ltr),
        convert(WritingMode::VerticalRl, Direction::Ltr)
    );
}

#[test]
fn vertical_lr_and_sideways_lr() {
    assert_eq!(
        convert(WritingMode::VerticalLr, Direction::Ltr),
        PhysicalRect {
            x: 20.0,
            y: 10.0,
            width: 40.0,
            height: 30.0
        }
    );
    // sideways-lr: inline direction runs bottom to top for LTR.
    assert_eq!(
        convert(WritingMode::SidewaysLr, Direction::Ltr),
        PhysicalRect {
            x: 20.0,
            y: 60.0,
            width: 40.0,
            height: 30.0
        }
    );
    assert_eq!(
        convert(WritingMode::SidewaysLr, Direction::Rtl),
        PhysicalRect {
            x: 20.0,
            y: 10.0,
            width: 40.0,
            height: 30.0
        }
    );
}

#[test]
fn vertical_predicate() {
    assert!(!WritingMode::HorizontalTb.is_vertical());
    assert!(WritingMode::VerticalRl.is_vertical());
    assert!(WritingMode::SidewaysLr.is_vertical());
}
