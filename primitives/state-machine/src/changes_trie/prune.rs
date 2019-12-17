// Copyright 2017-2019 Parity Technologies (UK) Ltd.
// This file is part of Substrate.

// Substrate is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Substrate is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Substrate.  If not, see <http://www.gnu.org/licenses/>.

//! Changes trie pruning-related functions.

use hash_db::Hasher;
use sp_trie::Recorder;
use log::warn;
use num_traits::One;
use crate::proving_backend::ProvingBackendRecorder;
use crate::trie_backend_essence::TrieBackendEssence;
use crate::changes_trie::{AnchorBlockId, Storage, BlockNumber};
use crate::changes_trie::storage::TrieBackendAdapter;
use crate::changes_trie::input::{ChildIndex, InputKey};
use codec::{Decode, Codec};

/// Prune obsolete changes tries. Pruning happens at the same block, where highest
/// level digest is created. Pruning guarantees to save changes tries for last
/// `min_blocks_to_keep` blocks. We only prune changes tries at `max_digest_interval`
/// ranges.
pub fn prune<H: Hasher, Number: BlockNumber, F: FnMut(H::Out)>(
	storage: &dyn Storage<H, Number>,
	first: Number,
	last: Number,
	current_block: &AnchorBlockId<H::Out, Number>,
	mut remove_trie_node: F,
) where H::Out: Codec {
	// delete changes trie for every block in range
	let mut block = first;
	loop {
		if block >= last.clone() + One::one() {
			break;
		}

		let prev_block = block.clone();
		block += One::one();

		let block = prev_block;
		let root = match storage.root(current_block, block.clone()) {
			Ok(Some(root)) => root,
			Ok(None) => continue,
			Err(error) => {
				// try to delete other tries
				warn!(target: "trie", "Failed to read changes trie root from DB: {}", error);
				continue;
			},
		};
		let children_roots = {
			let trie_storage = TrieBackendEssence::<_, H>::new(
				crate::changes_trie::TrieBackendStorageAdapter(storage),
				root,
			);
			let child_prefix = ChildIndex::key_neutral_prefix(block.clone());
			let mut children_roots = Vec::new();
			trie_storage.for_key_values_with_prefix(&child_prefix, |key, value| {
				if let Ok(InputKey::ChildIndex::<Number>(_trie_key)) = Decode::decode(&mut &key[..]) {
					if let Ok(value) = <Vec<u8>>::decode(&mut &value[..]) {
						let mut trie_root = <H as Hasher>::Out::default();
						trie_root.as_mut().copy_from_slice(&value[..]);
						children_roots.push(trie_root);
					}
				}
			});

			children_roots
		};
		for root in children_roots.into_iter() {
			prune_trie(storage, root, &mut remove_trie_node);
		}

		prune_trie(storage, root, &mut remove_trie_node);
	}
}

// Prune a trie.
fn prune_trie<H: Hasher, Number: BlockNumber, F: FnMut(H::Out)>(
	storage: &dyn Storage<H, Number>,
	root: H::Out,
	remove_trie_node: &mut F,
) where H::Out: Codec {

	// enumerate all changes trie' keys, recording all nodes that have been 'touched'
	// (effectively - all changes trie nodes)
	let mut proof_recorder: Recorder<H::Out> = Default::default();
	{
		let mut trie = ProvingBackendRecorder::<_, H> {
			backend: &TrieBackendEssence::new(TrieBackendAdapter::new(storage), root),
			proof_recorder: &mut proof_recorder,
		};
		trie.record_all_keys();
	}

	// all nodes of this changes trie should be pruned
	remove_trie_node(root);
	for node in proof_recorder.drain().into_iter().map(|n| n.hash) {
		remove_trie_node(node);
	}
}

#[cfg(test)]
mod tests {
	use std::collections::HashSet;
	use sp_trie::MemoryDB;
	use sp_core::{H256, Blake2Hasher};
	use crate::backend::insert_into_memory_db;
	use crate::changes_trie::storage::InMemoryStorage;
	use codec::Encode;
	use super::*;

	fn prune_by_collect(
		storage: &dyn Storage<Blake2Hasher, u64>,
		first: u64,
		last: u64,
		current_block: u64,
	) -> HashSet<H256> {
		let mut pruned_trie_nodes = HashSet::new();
		let anchor = AnchorBlockId { hash: Default::default(), number: current_block };
		prune(storage, first, last, &anchor,
			|node| { pruned_trie_nodes.insert(node); });
		pruned_trie_nodes
	}

	#[test]
	fn prune_works() {
		fn prepare_storage() -> InMemoryStorage<Blake2Hasher, u64> {

			let child_key = ChildIndex { block: 67u64, storage_key: b"1".to_vec() }.encode();
			let mut mdb1 = MemoryDB::<Blake2Hasher>::default();
			let root1 = insert_into_memory_db::<Blake2Hasher, _>(
				&mut mdb1, vec![(vec![10], vec![20])]).unwrap();
			let mut mdb2 = MemoryDB::<Blake2Hasher>::default();
			let root2 = insert_into_memory_db::<Blake2Hasher, _>(
				&mut mdb2,
				vec![(vec![11], vec![21]), (vec![12], vec![22])],
			).unwrap();
			let mut mdb3 = MemoryDB::<Blake2Hasher>::default();
			let ch_root3 = insert_into_memory_db::<Blake2Hasher, _>(
				&mut mdb3, vec![(vec![110], vec![120])]).unwrap();
			let root3 = insert_into_memory_db::<Blake2Hasher, _>(&mut mdb3, vec![
				(vec![13], vec![23]),
				(vec![14], vec![24]),
				(child_key, ch_root3.as_ref().encode()),
			]).unwrap();
			let mut mdb4 = MemoryDB::<Blake2Hasher>::default();
			let root4 = insert_into_memory_db::<Blake2Hasher, _>(
				&mut mdb4,
				vec![(vec![15], vec![25])],
			).unwrap();
			let storage = InMemoryStorage::new();
			storage.insert(65, root1, mdb1);
			storage.insert(66, root2, mdb2);
			storage.insert(67, root3, mdb3);
			storage.insert(68, root4, mdb4);

			storage
		}

		let storage = prepare_storage();
		assert!(prune_by_collect(&storage, 20, 30, 90).is_empty());
		assert!(!storage.into_mdb().drain().is_empty());

		let storage = prepare_storage();
		let prune60_65 = prune_by_collect(&storage, 60, 65, 90);
		assert!(!prune60_65.is_empty());
		storage.remove_from_storage(&prune60_65);
		assert!(!storage.into_mdb().drain().is_empty());

		let storage = prepare_storage();
		let prune60_70 = prune_by_collect(&storage, 60, 70, 90);
		assert!(!prune60_70.is_empty());
		storage.remove_from_storage(&prune60_70);
		assert!(storage.into_mdb().drain().is_empty());
	}
}
