use super::*;
use crate::bingo::model::{
    CELL_COUNT, Card, EntryNumber, GameState, ImportedCell, KnownUser, MAX_GAME_DESCRIPTION_CHARS,
    Position, REQUIRED_ENTRIES,
};
use claims::{assert_err, assert_ok, assert_ok_eq, assert_some};
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
    assert_ok!(Position::try_from(index))
}

async fn store() -> BingoStore {
    assert_ok!(BingoStore::connect("sqlite::memory:").await)
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
    assert_ok!(store.generate_card(CHAT_ID, None, owner, false).await)
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
    let free = assert_some!(card.cells.get(Position::FREE.index()));
    assert!(free.is_free);
    assert!(free.marked);
    let fetched = assert_ok!(store.card(CHAT_ID, None, owner.user_id).await);
    assert_eq!(fetched.cells, card.cells);
}

#[tokio::test]
async fn imports_entry_files_atomically_and_deduplicates_entries() {
    let store = store().await;
    assert_ok!(
        store
            .create_game(CHAT_ID, "season", "Season", UserId(10))
            .await
    );
    assert_ok_eq!(
        store
            .import_entries(
                CHAT_ID,
                "season",
                &["Safety car".to_owned(), "safety   car".to_owned()],
            )
            .await,
        1
    );

    let invalid = vec!["Wet race".to_owned(), "x".repeat(129)];
    assert_err!(store.import_entries(CHAT_ID, "season", &invalid).await);
    let (_, entries) = assert_ok!(store.list_entries(CHAT_ID, Some("season")).await);
    assert_eq!(entries.len(), 1);
}

#[tokio::test]
async fn numbers_entries_independently_for_each_game() {
    let store = store().await;
    for slug in ["season", "sprint"] {
        assert_ok!(store.create_game(CHAT_ID, slug, slug, UserId(10)).await);
        let first = assert_ok!(store.add_entry(CHAT_ID, Some(slug), "First").await);
        let second = assert_ok!(store.add_entry(CHAT_ID, Some(slug), "Second").await);
        assert_eq!(first.number, assert_ok!(EntryNumber::try_from(1)));
        assert_eq!(second.number, assert_ok!(EntryNumber::try_from(2)));
    }

    let first = assert_ok!(EntryNumber::try_from(1));
    assert_ok!(store.delete_entry(CHAT_ID, Some("season"), first).await);
    let (_, sprint_entries) = assert_ok!(store.list_entries(CHAT_ID, Some("sprint")).await);
    assert_eq!(sprint_entries.len(), 2);

    let restored = assert_ok!(store.add_entry(CHAT_ID, Some("season"), "First").await);
    assert_eq!(restored.number, first);
}

#[tokio::test]
async fn stores_updates_and_clears_game_descriptions() {
    let store = store().await;
    assert_ok!(
        store
            .create_game(CHAT_ID, "season", "Season", UserId(10))
            .await
    );
    let described = assert_ok!(
        store
            .set_game_description(CHAT_ID, "season", "  Welcome to 2026.  ")
            .await
    );
    assert_eq!(described.description, "Welcome to 2026.");
    assert_eq!(
        assert_ok!(store.game(CHAT_ID, Some("season")).await).description,
        "Welcome to 2026."
    );
    assert_err!(
        store
            .set_game_description(
                CHAT_ID,
                "season",
                &"x".repeat(MAX_GAME_DESCRIPTION_CHARS + 1),
            )
            .await
    );
    let cleared = assert_ok!(store.set_game_description(CHAT_ID, "season", "").await);
    assert!(cleared.description.is_empty());
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
async fn rejects_card_reset_after_game_is_closed() {
    let store = store().await;
    let owner = user(10, "driver");
    let card = setup_card(&store, &owner).await;
    assert_ok!(
        store
            .toggle_cell(card.id, owner.user_id, position(0), false)
            .await
    );
    assert_ok!(
        store
            .set_game_state(CHAT_ID, "season", GameState::Closed)
            .await
    );

    assert_err!(store.reset_card(CHAT_ID, None, owner.user_id).await);
    assert_err!(
        store
            .toggle_cell(card.id, UserId(99), position(1), true)
            .await
    );
    let fetched = assert_ok!(store.card(CHAT_ID, None, owner.user_id).await);
    assert!(assert_some!(fetched.cells.first()).marked);
}

#[tokio::test]
async fn owners_and_administrators_can_mark_and_first_line_is_announced_once() {
    let store = store().await;
    let owner = user(10, "driver");
    let card = setup_card(&store, &owner).await;

    assert_err!(
        store
            .toggle_cell(card.id, UserId(99), position(0), false)
            .await
    );
    assert_ok!(
        store
            .toggle_cell(card.id, UserId(99), position(0), true)
            .await
    );
    assert_ok!(
        store
            .toggle_cell(card.id, UserId(99), position(0), true)
            .await
    );
    for index in 0..4 {
        let toggle = assert_ok!(
            store
                .toggle_cell(card.id, owner.user_id, position(index), false)
                .await
        );
        assert!(!toggle.newly_completed);
    }
    let toggle = assert_ok!(
        store
            .toggle_cell(card.id, owner.user_id, position(4), false)
            .await
    );
    assert!(toggle.newly_completed);
    assert!(toggle.card.has_bingo());

    assert_ok!(
        store
            .toggle_cell(card.id, owner.user_id, position(4), false)
            .await
    );
    let repeated = assert_ok!(
        store
            .toggle_cell(card.id, owner.user_id, position(4), false)
            .await
    );
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
    let (_, entries) = assert_ok!(store.list_entries(CHAT_ID, None).await);
    let entry = assert_some!(entries.first());
    assert_ok!(
        store
            .edit_entry(CHAT_ID, Some("season"), entry.number, "Changed entry")
            .await
    );

    let fetched = assert_ok!(store.card(CHAT_ID, None, owner.user_id).await);
    assert_eq!(
        fetched
            .cells
            .iter()
            .map(|cell| cell.text.clone())
            .collect::<Vec<_>>(),
        original_texts
    );
}

#[tokio::test]
async fn sets_card_cells_from_game_scoped_entry_numbers() {
    let store = store().await;
    let owner = user(10, "driver");
    let card = setup_card(&store, &owner).await;
    let (_, entries) = assert_ok!(store.list_entries(CHAT_ID, Some("season")).await);
    let current_text = &assert_some!(card.cells.first()).text;
    let entry = assert_some!(entries.iter().find(|entry| entry.text != *current_text));
    let changed = assert_ok!(
        store
            .set_card_cell(CHAT_ID, "season", owner.user_id, position(0), entry.number,)
            .await
    );
    assert_eq!(assert_some!(changed.cells.first()).text, entry.text);

    assert_ok!(
        store
            .create_game(CHAT_ID, "sprint", "Sprint", owner.user_id)
            .await
    );
    let other_entry = assert_ok!(
        store
            .add_entry(CHAT_ID, Some("sprint"), "Sprint-only entry")
            .await
    );
    assert_eq!(other_entry.number, assert_ok!(EntryNumber::try_from(1)));
    let changed = assert_ok!(
        store
            .set_card_cell(
                CHAT_ID,
                "season",
                owner.user_id,
                position(1),
                other_entry.number,
            )
            .await
    );
    assert_eq!(assert_some!(changed.cells.get(1)).text, "Entry 0");
    assert_ne!(assert_some!(changed.cells.get(1)).text, other_entry.text);
}

#[tokio::test]
async fn imports_cards_from_game_scoped_entry_numbers() {
    let store = store().await;
    let owner = user(10, "driver");
    let original = setup_card(&store, &owner).await;
    let (_, entries) = assert_ok!(store.list_entries(CHAT_ID, Some("season")).await);
    let mut entries = entries.iter();
    let imported = Position::iter()
        .map(|cell_position| ImportedCell {
            entry_number: (cell_position != Position::FREE)
                .then(|| entries.next().map(|entry| entry.number))
                .flatten(),
            marked: cell_position == position(0) || cell_position == Position::FREE,
            is_free: cell_position == Position::FREE,
        })
        .collect::<Vec<_>>();
    let card = assert_ok!(
        store
            .import_card(CHAT_ID, "season", &owner, &imported, true)
            .await
    );
    let first = assert_some!(card.cells.first());
    assert!(first.marked);
    assert_eq!(first.text, "Entry 0");
    assert!(assert_some!(card.cells.get(Position::FREE.index())).is_free);

    let mut invalid = imported;
    assert_some!(invalid.first_mut()).entry_number =
        Some(assert_ok!(EntryNumber::try_from(i64::MAX)));
    assert_err!(
        store
            .import_card(CHAT_ID, "season", &owner, &invalid, true)
            .await
    );
    assert_ok_eq!(
        store.card(CHAT_ID, Some("season"), owner.user_id).await,
        card
    );
    assert_ne!(card.id, original.id);
}
