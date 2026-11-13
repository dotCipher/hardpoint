//! A keyboard's geometry: how key numbers, the switch matrix and the lights
//! line up. It is always built from a device card, never written in code.

use crate::catalog::Card;
use anyhow::{anyhow, bail, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    /// Key number, in the numbering the card declares.
    pub index: u8,
    /// Switch matrix position, as reported in key events.
    pub row: u8,
    pub col: u8,
    /// Position in the light chain, as addressed by lighting commands.
    pub led: u8,
    /// Grid cell used when drawing the board.
    pub x: u8,
    pub y: u8,
}

#[derive(Clone, Debug)]
pub struct Board {
    /// The card this board came from, for example `vendor/model`.
    pub card: String,
    pub name: String,
    pub vendor_id: u16,
    pub product_ids: Vec<u16>,
    pub keys: Vec<Key>,
}

impl Board {
    pub fn from_card(card: &Card) -> Result<Board> {
        let layout = card.keys.as_ref().ok_or_else(|| anyhow!("{} has no key layout", card.device.id))?;
        let ids: Vec<(u16, u16)> =
            card.matches.iter().filter_map(|m| Some((m.vendor_id?, m.product_id?))).collect();
        let Some((vendor_id, _)) = ids.first().copied() else {
            bail!("{} has no vendor and product id to find it by", card.device.id);
        };
        let keys: Vec<Key> = (0..layout.matrix.len())
            .map(|i| Key {
                index: i as u8,
                row: layout.matrix[i][0],
                col: layout.matrix[i][1],
                led: layout.leds[i],
                x: layout.grid[i][0],
                y: layout.grid[i][1],
            })
            .collect();
        let count = keys.len();
        let distinct = |values: Vec<(u8, u8)>| {
            let mut v = values;
            v.sort();
            v.dedup();
            v.len() == count
        };
        if count == 0 || count > 255 {
            bail!("{} must describe between 1 and 255 keys", card.device.id);
        }
        if !distinct(keys.iter().map(|k| (k.row, k.col)).collect())
            || !distinct(keys.iter().map(|k| (k.led, 0)).collect())
            || !distinct(keys.iter().map(|k| (k.x, k.y)).collect())
        {
            bail!("{}: two keys share a switch position, a light or a drawing cell", card.device.id);
        }
        if keys.iter().any(|k| k.led as usize >= count) {
            bail!("{}: a light number is larger than the number of keys", card.device.id);
        }
        Ok(Board {
            card: card.device.id.clone(),
            name: format!("{} {}", card.device.vendor, card.device.name),
            vendor_id,
            product_ids: ids.iter().filter(|(v, _)| *v == vendor_id).map(|(_, p)| *p).collect(),
            keys,
        })
    }

    pub fn by_index(&self, index: u8) -> Option<&Key> {
        self.keys.get(index as usize)
    }

    pub fn by_matrix(&self, row: u8, col: u8) -> Option<&Key> {
        self.keys.iter().find(|k| k.row == row && k.col == col)
    }

    pub fn led_count(&self) -> usize {
        self.keys.len()
    }
}

/// The first keyboard in the repository's catalog, for tests.
#[cfg(test)]
pub fn test_board() -> Board {
    let catalog = crate::catalog::repo_catalog();
    let card = catalog.cards.iter().find(|c| c.keys.is_some()).expect("a keyboard card");
    Board::from_card(card).expect("the keyboard card describes a valid board")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::repo_catalog;

    #[test]
    fn every_keyboard_card_describes_a_consistent_board() {
        let catalog = repo_catalog();
        let keyboards: Vec<_> = catalog.cards.iter().filter(|c| c.keys.is_some()).collect();
        assert!(!keyboards.is_empty());
        for card in keyboards {
            let board = Board::from_card(card).unwrap_or_else(|e| panic!("{e:#}"));
            for key in &board.keys {
                assert_eq!(board.by_matrix(key.row, key.col), Some(key));
                assert_eq!(board.by_index(key.index), Some(key));
            }
        }
    }

    #[test]
    fn a_card_with_two_keys_on_one_light_is_rejected() {
        let catalog = repo_catalog();
        let mut card = catalog.cards.iter().find(|c| c.keys.is_some()).unwrap().clone();
        let keys = card.keys.as_mut().unwrap();
        keys.leds[1] = keys.leds[0];
        assert!(Board::from_card(&card).is_err());
    }

    #[test]
    fn a_card_without_a_layout_is_not_a_board() {
        let catalog = repo_catalog();
        let card = catalog.cards.iter().find(|c| c.keys.is_none()).expect("a card with no keys");
        assert!(Board::from_card(card).is_err());
    }
}
