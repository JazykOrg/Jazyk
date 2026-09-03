// The walk endpoints: one card per entity and one page per diagram, served exactly
// as the shared model (card.rs) serializes them. The GUI explorer, docsgen, and the
// LSP hover read the same structs, so nothing is re-derived here.
// Mirrors docs/frontends/gui.md#api and docs/frontends/gui.md#explore.
use super::state::SharedState;
use crate::card::{entity_card, view_page, Walk};
use crate::store::Store;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde_json::Value;

// The entity's card as JSON: None when no entity has the id, redirects followed.
pub fn card_value(store: &Store, id: &str) -> Option<Value> {
    let walk = Walk::new(store);
    entity_card(store, &walk, id).map(|c| serde_json::to_value(c).expect("a card serializes"))
}

// The view's diagram page as JSON: None when no view has the id.
pub fn page_value(store: &Store, id: &str) -> Option<Value> {
    let walk = Walk::new(store);
    view_page(store, &walk, id).map(|p| serde_json::to_value(p).expect("a page serializes"))
}

pub async fn entity_card_handler(
    State(st): State<SharedState>,
    UrlPath(id): UrlPath<String>,
) -> Response {
    let store = super::api::load_store(&st).await;
    match card_value(&store, &id) {
        Some(v) => Json(v).into_response(),
        None => super::api::err(StatusCode::NOT_FOUND, format!("no entity {}", id)),
    }
}

pub async fn view_page_handler(
    State(st): State<SharedState>,
    UrlPath(id): UrlPath<String>,
) -> Response {
    let store = super::api::load_store(&st).await;
    match page_value(&store, &id) {
        Some(v) => Json(v).into_response(),
        None => super::api::err(StatusCode::NOT_FOUND, format!("no view {}", id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::RecordBatch;

    fn store() -> Store {
        let mut s = crate::derive::tests::showcase_store();
        crate::derive::recompute(&mut s, "g1", &mut RecordBatch::new(1));
        s
    }

    // The card JSON carries exactly the field names docs/frontends/gui.md#api lists,
    // camel-cased where the shared model renames them, and one level in every
    // direction as ids the client can walk.
    #[test]
    fn card_value_serializes_the_shared_model_field_for_field() {
        let s = store();
        let c = card_value(&s, "ent:order-service").expect("the order service's card");
        let keys: Vec<&str> = c.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "name",
                "stereotype",
                "definition",
                "provenance",
                "breadcrumb",
                "context",
                "inside",
                "insideFlows",
                "relationships",
                "flows",
                "siblings",
                "children",
                "requirementCount",
                "proposal",
            ]
        );
        assert_eq!(c["id"], "ent:order-service");
        assert_eq!(c["provenance"], "quote");
        assert_eq!(c["context"], "view:component/shop");
        assert_eq!(c["inside"], "view:component/order-service");
        let crumbs: Vec<&str> = c["breadcrumb"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["id"].as_str().unwrap())
            .collect();
        assert_eq!(crumbs, vec!["scope:public", "ent:shop", "ent:order-service"]);
        assert_eq!(c["breadcrumb"][0]["name"], "Public");
        let child = c["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["id"] == "ent:order")
            .expect("the order under the service");
        assert!(child["childCount"].is_number());
        let rel = c["relationships"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["other"] == "ent:inventory-service")
            .expect("the dependency on the stock api, lifted to the inventory service");
        assert_eq!(rel["type"], "dependency");
        assert!(matches!(rel["direction"].as_str(), Some("a" | "b" | "both")));
        assert!(rel["count"].as_u64().unwrap() >= 1);
        assert!(c["requirementCount"].as_u64().is_some());
        assert!(c["proposal"].is_null());
        // A leaf: null inside, no children, nothing invented.
        let leaf = card_value(&s, "ent:order-item").expect("the order item's card");
        assert!(leaf["inside"].is_null());
        assert!(leaf["children"].as_array().unwrap().is_empty());
        assert!(leaf["insideFlows"].as_array().unwrap().is_empty());
        // An unknown id is absence, the handler's 404.
        assert!(card_value(&s, "ent:nobody").is_none());
    }

    // The page JSON: the level with its breadcrumb, the legend in drawing order with
    // the level below marked, the steps only on flow kinds, and the views around.
    #[test]
    fn page_value_serializes_level_legend_steps_and_around() {
        let s = store();
        let p = page_value(&s, "view:component/shop").expect("the shop level's page");
        let keys: Vec<&str> = p.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["id", "kind", "title", "level", "drawn", "steps", "around"]);
        assert_eq!(p["level"]["target"], "ent:shop");
        assert_eq!(p["level"]["breadcrumb"][0]["id"], "scope:public");
        let drawn = p["drawn"].as_array().unwrap();
        let order = drawn
            .iter()
            .find(|d| d["id"] == "ent:order-service")
            .expect("the order service drawn");
        assert_eq!(order["levelView"], "view:component/order-service");
        assert!(p["steps"].as_array().unwrap().is_empty());
        assert_eq!(p["around"]["above"], "view:component/public");
        assert!(p["around"]["below"]
            .as_array()
            .unwrap()
            .contains(&Value::from("view:component/order-service")));
        assert!(p["around"]["sameLevel"]
            .as_array()
            .unwrap()
            .contains(&Value::from("view:usecase/shop-customer-shop")));
        let f = page_value(&s, "view:sequence/shop-customer-shop").expect("the sequence's page");
        let steps = f["steps"].as_array().unwrap();
        assert!(!steps.is_empty());
        assert_eq!(steps[0]["requirement"], "req:shop-1");
        assert!(steps[0]["from"].is_string() && steps[0]["to"].is_string());
        assert!(page_value(&s, "view:component/nowhere").is_none());
    }
}
