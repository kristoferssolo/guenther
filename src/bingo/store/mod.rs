mod card;
mod connection;
mod entry;
mod game;
mod id;
mod user;
mod validation;

use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct BingoStore {
    pool: SqlitePool,
}

#[cfg(test)]
mod tests {
    use crate::bingo::{
        model::{
            CELL_COUNT, Card, GameState, KnownUser, MAX_GAME_DESCRIPTION_CHARS, Position,
            REQUIRED_ENTRIES,
        },
        store::BingoStore,
    };
    use claims::{assert_err, assert_ok, assert_ok_eq};
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
        let (_, entries) = store
            .list_entries(CHAT_ID, Some("season"))
            .await
            .expect("list imported entries");
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn stores_updates_and_clears_game_descriptions() {
        let store = store().await;
        assert_ok!(
            store
                .create_game(CHAT_ID, "season", "Season", UserId(10))
                .await
        );
        let described = store
            .set_game_description(CHAT_ID, "season", "  Welcome to 2026.  ")
            .await
            .expect("set game description");
        assert_eq!(described.description, "Welcome to 2026.");
        assert_eq!(
            store
                .game(CHAT_ID, Some("season"))
                .await
                .expect("fetch described game")
                .description,
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
        let cleared = store
            .set_game_description(CHAT_ID, "season", "")
            .await
            .expect("clear game description");
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

    #[tokio::test]
    async fn sets_card_cells_from_active_game_entry_ids() {
        let store = store().await;
        let owner = user(10, "driver");
        let card = setup_card(&store, &owner).await;
        let (_, entries) = store
            .list_entries(CHAT_ID, Some("season"))
            .await
            .expect("list game entries");
        let entry = entries
            .iter()
            .find(|entry| entry.text != card.cells[0].text)
            .expect("another active entry is available");
        let changed = store
            .set_card_cell(CHAT_ID, "season", owner.user_id, position(0), entry.id)
            .await
            .expect("set card cell from entry ID");
        assert_eq!(changed.cells[0].text, entry.text);

        assert_ok!(
            store
                .create_game(CHAT_ID, "sprint", "Sprint", owner.user_id)
                .await
        );
        let other_entry = store
            .add_entry(CHAT_ID, Some("sprint"), "Sprint-only entry")
            .await
            .expect("add entry to other game");
        assert_err!(
            store
                .set_card_cell(
                    CHAT_ID,
                    "season",
                    owner.user_id,
                    position(1),
                    other_entry.id,
                )
                .await
        );
        assert_eq!(
            store
                .card(CHAT_ID, Some("season"), owner.user_id)
                .await
                .expect("fetch unchanged card")
                .cells[1],
            card.cells[1]
        );
    }
}
