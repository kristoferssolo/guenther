use super::*;
use crate::bingo::model::{CELL_COUNT, Card, GameState, KnownUser, Position, REQUIRED_ENTRIES};
use claims::{assert_err, assert_ok};
use teloxide::types::{ChatId, UserId};

const CHAT_ID: ChatId = ChatId(1);

fn user(id: u64, username: &str) -> KnownUser {
    KnownUser {
        user_id: UserId(id),
        username: Some(username.to_owned()),
        display_name: username.to_owned(),
    }
}

fn position(index: usize) -> Position {
    Position::try_from(index).expect("test position is valid")
}

async fn store() -> BingoStore {
    BingoStore::connect("sqlite::memory:")
        .await
        .expect("create in-memory bingo store")
}

async fn setup_card(store: &BingoStore, owner: &KnownUser) -> Card {
    assert_ok!(store.observe_user(CHAT_ID, owner).await);
    assert_ok!(
        store
            .create_game(CHAT_ID, "season", "Season", owner.user_id)
            .await
    );
    assert_ok!(
        store
            .set_game_state(CHAT_ID, "season", GameState::Active)
            .await
    );
    for index in 0..REQUIRED_ENTRIES {
        assert_ok!(
            store
                .add_entry(CHAT_ID, None, &format!("Entry {index}"))
                .await
        );
    }
    store
        .generate_card(CHAT_ID, None, owner, false)
        .await
        .expect("generate card")
}

#[tokio::test]
async fn rejects_numeric_game_slugs() {
    let store = store().await;
    assert_err!(
        store
            .create_game(CHAT_ID, "2026", "Season", UserId(10))
            .await
    );
}

#[tokio::test]
async fn generates_persistent_card_with_free_center() {
    let store = store().await;
    let owner = user(10, "driver");
    let card = setup_card(&store, &owner).await;
    assert_eq!(card.cells.len(), CELL_COUNT);
    assert!(card.cells[Position::FREE.index()].is_free);
    assert!(card.cells[Position::FREE.index()].marked);
    let fetched = store
        .card(CHAT_ID, None, owner.user_id)
        .await
        .expect("fetch card");
    assert_eq!(fetched.cells, card.cells);
}

#[tokio::test]
async fn rejects_duplicate_generation_without_replace() {
    let store = store().await;
    let owner = user(10, "driver");
    setup_card(&store, &owner).await;
    assert_err!(store.generate_card(CHAT_ID, None, &owner, false).await);
    assert_ok!(store.generate_card(CHAT_ID, None, &owner, true).await);
}

#[tokio::test]
async fn only_owner_can_mark_and_first_line_is_announced_once() {
    let store = store().await;
    let owner = user(10, "driver");
    let card = setup_card(&store, &owner).await;

    assert_err!(store.toggle_cell(card.id, UserId(99), position(0)).await);
    for index in 0..4 {
        let toggle = store
            .toggle_cell(card.id, owner.user_id, position(index))
            .await
            .expect("mark cell");
        assert!(!toggle.newly_completed);
    }
    let toggle = store
        .toggle_cell(card.id, owner.user_id, position(4))
        .await
        .expect("complete row");
    assert!(toggle.newly_completed);
    assert!(toggle.card.has_bingo());

    assert_ok!(store.toggle_cell(card.id, owner.user_id, position(4)).await);
    let repeated = store
        .toggle_cell(card.id, owner.user_id, position(4))
        .await
        .expect("complete same row again");
    assert!(!repeated.newly_completed);
}

#[tokio::test]
async fn edits_do_not_change_existing_card_snapshots() {
    let store = store().await;
    let owner = user(10, "driver");
    let card = setup_card(&store, &owner).await;
    let original_texts = card
        .cells
        .iter()
        .map(|cell| cell.text.clone())
        .collect::<Vec<_>>();
    let (_, entries) = store
        .list_entries(CHAT_ID, None)
        .await
        .expect("list entries");
    assert_ok!(
        store
            .edit_entry(CHAT_ID, entries[0].id, "Changed entry")
            .await
    );

    let fetched = store
        .card(CHAT_ID, None, owner.user_id)
        .await
        .expect("fetch card");
    assert_eq!(
        fetched
            .cells
            .iter()
            .map(|cell| cell.text.clone())
            .collect::<Vec<_>>(),
        original_texts
    );
}
