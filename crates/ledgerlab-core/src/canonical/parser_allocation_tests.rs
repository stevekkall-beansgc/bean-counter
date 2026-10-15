use super::*;

#[derive(Default)]
struct Recorder {
    before: Vec<ParseAllocation>,
    finished: Vec<(ParseAllocation, usize)>,
    remaining: Option<usize>,
    refuse: Option<&'static str>,
    fail_finish: bool,
}
fn refused() -> Error {
    Error::new(
        "TEST_ALLOCATION_REFUSED",
        "distinct caller allocation error",
    )
}
impl ParseAllocationWork for Recorder {
    fn before_allocation(&mut self, request: ParseAllocation) -> Result<()> {
        self.before.push(request);
        let kind = match request {
            ParseAllocation::ArrayCapacity { .. } => "array",
            ParseAllocation::ObjectEntry { .. } => "object",
            ParseAllocation::StringDecode { .. } => "string",
        };
        if self.refuse == Some(kind) || self.remaining == Some(0) {
            return Err(refused());
        }
        if let Some(remaining) = &mut self.remaining {
            *remaining -= 1;
        }
        Ok(())
    }
    fn finish_allocation(&mut self, request: ParseAllocation, retained: usize) -> Result<()> {
        if self.fail_finish {
            return Err(refused());
        }
        self.finished.push((request, retained));
        Ok(())
    }
}

#[test]
fn parser_allocation_compatibility_keeps_unicode_numbers_depth_and_decoded_duplicates() {
    eprintln!(
        "PARSER-ALLOCATION-ABI Value={} String={} VecValue={} Map={} usize={}",
        std::mem::size_of::<Value>(),
        std::mem::size_of::<String>(),
        std::mem::size_of::<Vec<Value>>(),
        std::mem::size_of::<Map<String, Value>>(),
        std::mem::size_of::<usize>(),
    );
    let raw = br#"{"a":[true,null,-9007199254740991,9007199254740991],"s":"\"\\\/\b\f\n\r\t\u000b\ud83d\ude00","z":{}}"#;
    let mut recorder = Recorder::default();
    let actual = parse_bounded_with_allocations(raw, raw.len(), &mut recorder).unwrap();
    assert_eq!(actual, parse_bounded(raw, raw.len()).unwrap());
    assert_eq!(actual["s"], json_string("\"\\/\u{8}\u{c}\n\r\t\u{b}😀"));
    assert_eq!(actual["a"][2], Value::from(-SAFE));
    for (raw, code) in [
        (br#"{"a":1,"\u0061":2}"#.as_slice(), "JSON_DUPLICATE_KEY"),
        (br#""\ud800""#.as_slice(), "JSON"),
        (br#""\udc00""#.as_slice(), "JSON"),
        (br#""\x""#.as_slice(), "JSON"),
        (b"\"\xff\"".as_slice(), "JSON"),
        (b"[01]".as_slice(), "JSON_NUMBER"),
        (b"[-0]".as_slice(), "JSON_NUMBER"),
        (b"[1e0]".as_slice(), "JSON_NUMBER"),
        (b"[9007199254740992]".as_slice(), "JSON_NUMBER"),
        (b"[1,]".as_slice(), "JSON"),
        (b"{}true".as_slice(), "JSON"),
    ] {
        let expected = parse_bounded(raw, raw.len()).unwrap_err();
        assert_eq!(expected.code, code);
        assert_eq!(
            parse_bounded_with_allocations(raw, raw.len(), &mut Recorder::default()).unwrap_err(),
            expected
        );
    }
    let deepest = format!("{}0{}", "[".repeat(32), "]".repeat(32));
    assert!(
        parse_bounded_with_allocations(deepest.as_bytes(), deepest.len(), &mut recorder).is_ok()
    );
    let too_deep = format!("[{}]", deepest);
    assert_eq!(
        parse_bounded_with_allocations(too_deep.as_bytes(), too_deep.len(), &mut recorder)
            .unwrap_err(),
        Error::new("LIMIT", "JSON depth")
    );
    let mut untouched = Recorder::default();
    assert_eq!(
        parse_bounded_with_allocations(raw, raw.len() - 1, &mut untouched)
            .unwrap_err()
            .code,
        "LIMIT"
    );
    assert!(untouched.before.is_empty());
}
fn json_string(s: &str) -> Value {
    Value::String(s.into())
}

#[test]
fn parser_allocation_refuses_before_dense_array_growth_object_insert_and_string_decode() {
    // Under4MiB encoded JSON can request much larger Value storage. Refusal at
    // the first capacity request leaves no admitted array growth or later walk.
    let raw = format!("[{}0]", "0,".repeat(200_000));
    let mut array = Recorder {
        refuse: Some("array"),
        ..Recorder::default()
    };
    assert_eq!(
        parse_bounded_with_allocations(raw.as_bytes(), BUNDLE_LIMIT, &mut array).unwrap_err(),
        refused()
    );
    assert_eq!(
        array.before,
        [ParseAllocation::ArrayCapacity {
            current_capacity: 0,
            requested_capacity: 4,
            element_size: std::mem::size_of::<Value>()
        }]
    );
    assert!(array.finished.is_empty());

    let mut string = Recorder {
        refuse: Some("string"),
        ..Recorder::default()
    };
    let raw = format!("\"{}\"", "\\u0061".repeat(100_000));
    assert_eq!(
        parse_bounded_with_allocations(raw.as_bytes(), BUNDLE_LIMIT, &mut string).unwrap_err(),
        refused()
    );
    assert_eq!(
        string.before,
        [ParseAllocation::StringDecode {
            encoded_bytes: raw.len()
        }]
    );
    assert!(string.finished.is_empty()); // Decoder was never entered.

    let mut object = Recorder {
        refuse: Some("object"),
        ..Recorder::default()
    };
    assert_eq!(
        parse_bounded_with_allocations(br#"{"k":0}"#, 64, &mut object).unwrap_err(),
        refused()
    );
    assert_eq!(
        object.before.last(),
        Some(&ParseAllocation::ObjectEntry {
            current_len: 0,
            key_bytes: 1,
            value_size: std::mem::size_of::<Value>(),
            string_size: std::mem::size_of::<String>()
        })
    );
    assert_eq!(object.finished.len(), 1); // Key only; insert follows admission.
}

#[test]
fn parser_allocation_reports_actual_retained_capacity_and_exact_callback_refusal() {
    let raw = br#"{"k":["\u0061",0,1,2,3,4,5,6,7]}"#;
    let mut recorded = Recorder::default();
    let expected = parse_bounded_with_allocations(raw, raw.len(), &mut recorded).unwrap();
    for (request, bytes) in &recorded.finished {
        match *request {
            ParseAllocation::ArrayCapacity {
                requested_capacity,
                element_size,
                ..
            } => assert!(*bytes >= requested_capacity * element_size),
            ParseAllocation::StringDecode { encoded_bytes } => assert!(*bytes <= encoded_bytes),
            ParseAllocation::ObjectEntry { .. } => {
                panic!("opaque map capacity must not be fabricated")
            }
        }
    }
    assert!(recorded.finished.iter().any(|(request, _)| matches!(
        request,
        ParseAllocation::ArrayCapacity {
            current_capacity: 4,
            requested_capacity: 8,
            ..
        }
    )));
    let n = recorded.before.len();
    let mut exact = Recorder {
        remaining: Some(n),
        ..Recorder::default()
    };
    assert_eq!(
        parse_bounded_with_allocations(raw, raw.len(), &mut exact).unwrap(),
        expected
    );
    assert_eq!(exact.remaining, Some(0));
    let mut short = Recorder {
        remaining: Some(n - 1),
        ..Recorder::default()
    };
    assert_eq!(
        parse_bounded_with_allocations(raw, raw.len(), &mut short).unwrap_err(),
        refused()
    );
    let mut finish = Recorder {
        fail_finish: true,
        ..Recorder::default()
    };
    assert_eq!(
        parse_bounded_with_allocations(b"[0]", 3, &mut finish).unwrap_err(),
        refused()
    );
    assert!(finish.finished.is_empty()); // No parsed tree escapes a failed finish.
}
