use super::*;
#[test]
fn six_independent_alias_request_operation_manifest_receipt_bytes_and_refs() {
    let f: Value = serde_json::from_slice(include_bytes!("independent.json")).unwrap();
    let mut count = 0;
    for scenario in f["scenarios"].as_array().unwrap() {
        for op in scenario["operations"].as_array().unwrap() {
            let name = op["name"].as_str().unwrap();
            let vectors = scenario["vectors"].as_array().unwrap();
            let expected_op = vectors
                .iter()
                .find(|v| v["name"] == format!("{name}.operation"))
                .unwrap();
            let body = &expected_op["body"];
            let request = &op["request_evidence"]["normalized_request"];
            let req = request::Request::parse_agreement(&records::bytes(request).unwrap()).unwrap();
            let descriptors: Vec<RecordRef> =
                serde_json::from_value(op["descriptor_resolution"]["descriptors"].clone()).unwrap();
            let ev = evidence(&req, &request["payload"], &descriptors).unwrap();
            assert_eq!(ev, op["request_evidence"]);
            let at = Timestamp::parse(body["accepted_at"].as_str().unwrap()).unwrap();
            let results: Vec<RecordRef> =
                serde_json::from_value(body["result_refs"].clone()).unwrap();
            let own: RecordRef = serde_json::from_value(expected_op["ref"].clone()).unwrap();
            let members: Vec<RecordRef> =
                serde_json::from_value(op["manifest"]["members"].clone()).unwrap();
            let members = members.into_iter().filter(|r| *r != own).collect();
            // Independent authority and member operands are supplied data here;
            // actual owner/source/origin eligibility needs the public history.
            let actual = wrappers(
                &req,
                &ev,
                &body["authority_snapshot"],
                &at,
                body["commit_sequence"].as_str().unwrap().parse().unwrap(),
                &results,
                members,
            )
            .unwrap();
            assert_eq!(json!(actual.operation_ref), expected_op["ref"]);
            assert_eq!(actual.operation_bytes, records::bytes(body).unwrap());
            assert_eq!(
                actual.manifest_bytes,
                records::bytes(&op["manifest"]).unwrap()
            );
            assert_eq!(json!(actual.manifest_ref), op["manifest_ref"]);
            assert_eq!(
                actual.receipt_bytes,
                records::bytes(&op["receipt"]).unwrap()
            );
            assert_eq!(json!(actual.receipt_ref), op["receipt_ref"]);
            assert!(actual
                .members
                .contains(&serde_json::from_value(op["original_operation_ref"].clone()).unwrap()));
            count += 1;
        }
    }
    assert_eq!(count, 6);
}
