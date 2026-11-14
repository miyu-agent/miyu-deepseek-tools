use serde_json::json;

use super::{Moment, dumps, float_repr, key, repr, round, str_of, strip, truthy};

#[test]
fn floats_print_like_python_repr() {
    // 右边是 python3 -c 'print(repr(x))' 的输出。
    for (number, python) in [
        (100.0, "100.0"),
        (99.5, "99.5"),
        (1e-05, "1e-05"),
        (1.5e-05, "1.5e-05"),
        (1e16, "1e+16"),
        (1.5e16, "1.5e+16"),
        (123_456_789_012_345.6, "123456789012345.6"),
        (0.0001, "0.0001"),
        (0.00012, "0.00012"),
        (-0.0, "-0.0"),
        (0.1, "0.1"),
        (2.5e-07, "2.5e-07"),
        (1e22, "1e+22"),
        (12_345_678_901_234_567.0, "1.2345678901234568e+16"),
        (9_999_999_999_999_998.0, "9999999999999998.0"),
    ] {
        assert_eq!(float_repr(number), python, "{number}");
    }
}

#[test]
fn str_and_truth_follow_python() {
    assert_eq!(str_of(&json!(null)), "None");
    assert_eq!(str_of(&json!(true)), "True");
    assert_eq!(str_of(&json!(7)), "7");
    assert_eq!(str_of(&json!(100.0)), "100.0");
    assert_eq!(str_of(&json!("字")), "字");
    assert_eq!(
        repr(&json!(["a", 1, {"k": "it's"}])),
        "['a', 1, {'k': \"it's\"}]"
    );
    for falsy in [
        json!(null),
        json!(false),
        json!(0),
        json!(0.0),
        json!(""),
        json!([]),
        json!({}),
    ] {
        assert!(!truthy(&falsy), "{falsy}");
    }
    assert!(truthy(&json!("0")) && truthy(&json!(-1)) && truthy(&json!([0])));
}

#[test]
fn dictionary_keys_treat_1_1_0_and_true_alike() {
    assert_eq!(key(&json!(1)), key(&json!(1.0)));
    assert_eq!(key(&json!(1)), key(&json!(true)));
    assert_ne!(key(&json!(1)), key(&json!("1")));
    assert_ne!(key(&json!(1.5)), key(&json!(1)));
    assert_eq!(key(&json!([1])), None);
}

#[test]
fn strip_and_round_are_python_s() {
    assert_eq!(strip("\u{1c} 已定位 \u{3000}\n"), "已定位");
    assert_eq!(
        (round(2.5), round(1.5), round(0.5), round(59.98)),
        (2, 2, 0, 60)
    );
}

#[test]
fn json_dumps_spacing_and_floats() {
    assert_eq!(
        dumps(&json!({"ok": true, "a": [1, 2.0, null], "b": {"x": "\"\n\u{7f}中"}, "c": 1e-05})),
        "{\"ok\": true, \"a\": [1, 2.0, null], \"b\": {\"x\": \"\\\"\\n\u{7f}中\"}, \"c\": 1e-05}"
    );
}

#[test]
fn beijing_time_and_cpython_rounding() {
    // 2026-11-14 13:05:42.123456 北京时间。
    let now = Moment {
        seconds: 1_794_632_742,
        micros: 123_456,
    };
    assert_eq!(now.date_minute(), "2026-11-14 13:05");
    assert_eq!(now.iso_minute(), "2026-11-14T13:05+08:00");
    assert_eq!(
        Moment::from_timestamp(1_709_164_800.0).month_minute(),
        "02-29 08:00"
    );
    assert_eq!(
        Moment::from_timestamp(1_767_196_740.0).iso_minute(),
        "2025-12-31T23:59+08:00"
    );
    assert_eq!(
        Moment::from_timestamp(0.0).date_minute(),
        "1970-01-01 08:00"
    );
    let carried = Moment::from_timestamp(1_794_600_000.999_999_6);
    assert_eq!((carried.seconds, carried.micros), (1_794_600_001, 0));
    let start = Moment::from_timestamp(100.0);
    assert_eq!(start.seconds_until(Moment::from_timestamp(159.5)), 59);
    assert_eq!(start.seconds_until(Moment::from_timestamp(40.5)), -59);
}
