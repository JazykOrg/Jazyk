use product::account::{Account, CustomerKind};

#[test]
fn req_customer_1_f180c28a() {
    let person_account = Account::held_by_customer_with_orderly("Ada", CustomerKind::Person);
    assert_eq!(person_account.provider, "Orderly");
    assert_eq!(person_account.customer_name, "Ada");
    assert_eq!(person_account.customer_kind, CustomerKind::Person);

    let organization_account = Account::held_by_customer_with_orderly("Example Org", CustomerKind::Organization);
    assert_eq!(organization_account.provider, "Orderly");
    assert_eq!(organization_account.customer_kind, CustomerKind::Organization);
}

#[test]
fn req_customer_4_7f25c49a() {
    let mut account = Account::held_by_customer_with_orderly("Ada", CustomerKind::Person);
    account.add_order(1001, false);

    let deletion = account.delete();

    assert!(deletion.is_err());
    assert!(!account.is_deleted());
    assert_eq!(
        deletion.unwrap_err(),
        "Customer cannot delete their account while an Order is unpaid."
    );
}
