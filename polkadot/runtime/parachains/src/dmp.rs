// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Polkadot.

// Polkadot is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Polkadot is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Polkadot.  If not, see <http://www.gnu.org/licenses/>.

//! To prevent Out of Memory errors on the `DownwardMessageQueue`, an
//! exponential fee factor (`DeliveryFeeFactor`) is set. The fee factor
//! increments exponentially after the number of messages in the
//! `DownwardMessageQueue` passes a threshold. This threshold is set as:
//!
//! ```ignore
//! // Maximum max sized messages that can be send to
//! // the DownwardMessageQueue before it runs out of memory
//! max_messages = MAX_POSSIBLE_ALLOCATION / max_downward_message_size
//! threshold = max_messages / THRESHOLD_FACTOR
//! ```
//! Based on the THRESHOLD_FACTOR, the threshold is set as a fraction of the
//! total messages. The `DeliveryFeeFactor` increases for a message over the
//! threshold by:
//!
//! `DeliveryFeeFactor = DeliveryFeeFactor *
//! (EXPONENTIAL_FEE_BASE + MESSAGE_SIZE_FEE_BASE * encoded_message_size_in_KB)`
//!
//! And decreases when the number of messages in the `DownwardMessageQueue` fall
//! below the threshold by:
//!
//! `DeliveryFeeFactor = DeliveryFeeFactor / EXPONENTIAL_FEE_BASE`
//!
//! As an extra defensive measure, a `max_messages` hard
//! limit is set to the number of messages in the DownwardMessageQueue. Messages
//! that would increase the number of messages in the queue above this hard
//! limit are dropped.

use crate::{
	configuration::{self, HostConfiguration},
	initializer, paras, FeeTracker, GetMinFeeFactor,
};
use alloc::vec::Vec;
use core::fmt;
use frame_support::pallet_prelude::*;
use frame_system::pallet_prelude::BlockNumberFor;
use polkadot_primitives::{DownwardMessage, Hash, Id as ParaId, InboundDownwardMessage};
use sp_core::MAX_POSSIBLE_ALLOCATION;
use sp_runtime::{
	traits::{BlakeTwo256, Hash as HashT, SaturatedConversion},
	FixedU128,
};
use xcm::latest::SendError;

pub use pallet::*;

#[cfg(test)]
mod tests;

const THRESHOLD_FACTOR: u32 = 2;

/// An error sending a downward message.
#[cfg_attr(test, derive(Debug))]
pub enum QueueDownwardMessageError {
	/// The message being sent exceeds the configured max message size.
	ExceedsMaxMessageSize,
	/// The destination is unknown.
	Unroutable,
}

impl From<QueueDownwardMessageError> for SendError {
	fn from(err: QueueDownwardMessageError) -> Self {
		match err {
			QueueDownwardMessageError::ExceedsMaxMessageSize => SendError::ExceedsMaxMessageSize,
			QueueDownwardMessageError::Unroutable => SendError::Unroutable,
		}
	}
}

/// An error returned by [`Pallet::check_processed_downward_messages`] that indicates an acceptance
/// check didn't pass.
pub(crate) enum ProcessedDownwardMessagesAcceptanceErr {
	/// If there are pending messages then `processed_downward_messages` should be at least 1,
	AdvancementRule,
	/// `processed_downward_messages` should not be greater than the number of pending messages.
	Underflow { processed_downward_messages: u32, dmq_length: u32 },
}

impl fmt::Debug for ProcessedDownwardMessagesAcceptanceErr {
	fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
		use ProcessedDownwardMessagesAcceptanceErr::*;
		match *self {
			AdvancementRule => {
				write!(fmt, "DMQ is not empty, but processed_downward_messages is 0",)
			},
			Underflow { processed_downward_messages, dmq_length } => write!(
				fmt,
				"processed_downward_messages = {}, but dmq_length is only {}",
				processed_downward_messages, dmq_length,
			),
		}
	}
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;

	#[pallet::pallet]
	#[pallet::without_storage_info]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config + configuration::Config + paras::Config {}

	/// The downward messages addressed for a certain para.
	#[pallet::storage]
	pub type DownwardMessageQueues<T: Config> = StorageMap<
		_,
		Twox64Concat,
		ParaId,
		Vec<InboundDownwardMessage<BlockNumberFor<T>>>,
		ValueQuery,
	>;

	/// A mapping that stores the downward message queue MQC head for each para.
	///
	/// Each link in this chain has a form:
	/// `(prev_head, B, H(M))`, where
	/// - `prev_head`: is the previous head hash or zero if none.
	/// - `B`: is the relay-chain block number in which a message was appended.
	/// - `H(M)`: is the hash of the message being appended.
	#[pallet::storage]
	pub(crate) type DownwardMessageQueueHeads<T: Config> =
		StorageMap<_, Twox64Concat, ParaId, Hash, ValueQuery>;

	/// The factor to multiply the base delivery fee by.
	#[pallet::storage]
	pub(crate) type DeliveryFeeFactor<T: Config> =
		StorageMap<_, Twox64Concat, ParaId, FixedU128, ValueQuery, GetMinFeeFactor<Pallet<T>>>;
}
/// Routines and getters related to downward message passing.
impl<T: Config> Pallet<T> {
	/// Block initialization logic, called by initializer.
	pub(crate) fn initializer_initialize(_now: BlockNumberFor<T>) -> Weight {
		Weight::zero()
	}

	/// Block finalization logic, called by initializer.
	pub(crate) fn initializer_finalize() {}

	/// Called by the initializer to note that a new session has started.
	pub(crate) fn initializer_on_new_session(
		_notification: &initializer::SessionChangeNotification<BlockNumberFor<T>>,
		outgoing_paras: &[ParaId],
	) {
		Self::perform_outgoing_para_cleanup(outgoing_paras);
	}

	/// Iterate over all paras that were noted for offboarding and remove all the data
	/// associated with them.
	fn perform_outgoing_para_cleanup(outgoing: &[ParaId]) {
		for outgoing_para in outgoing {
			Self::clean_dmp_after_outgoing(outgoing_para);
		}
	}

	/// Remove all relevant storage items for an outgoing parachain.
	fn clean_dmp_after_outgoing(outgoing_para: &ParaId) {
		DownwardMessageQueues::<T>::remove(outgoing_para);
		DownwardMessageQueueHeads::<T>::remove(outgoing_para);
	}

	/// Determine whether enqueuing a downward message to a specific recipient para would result
	/// in an error. If this returns `Ok(())` the caller can be certain that a call to
	/// `queue_downward_message` with the same parameters will be successful.
	pub fn can_queue_downward_message(
		config: &HostConfiguration<BlockNumberFor<T>>,
		para: &ParaId,
		msg: &DownwardMessage,
	) -> Result<(), QueueDownwardMessageError> {
		let serialized_len = msg.len() as u32;
		if serialized_len > config.max_downward_message_size {
			return Err(QueueDownwardMessageError::ExceedsMaxMessageSize)
		}

		// Hard limit on Queue size
		if Self::dmq_length(*para) > Self::dmq_max_length(config.max_downward_message_size) {
			return Err(QueueDownwardMessageError::ExceedsMaxMessageSize)
		}

		// If the head exists, we assume the parachain is legit and exists.
		if !paras::Heads::<T>::contains_key(para) {
			return Err(QueueDownwardMessageError::Unroutable)
		}

		Ok(())
	}

	/// Enqueue a downward message to a specific recipient para.
	///
	/// When encoded, the message should not exceed the `config.max_downward_message_size`.
	/// Otherwise, the message won't be sent and `Err` will be returned.
	///
	/// It is possible to send a downward message to a non-existent para. That, however, would lead
	/// to a dangling storage. If the caller cannot statically prove that the recipient exists
	/// then the caller should perform a runtime check.
	pub fn queue_downward_message(
		config: &HostConfiguration<BlockNumberFor<T>>,
		para: ParaId,
		msg: DownwardMessage,
	) -> Result<(), QueueDownwardMessageError> {
		let serialized_len = msg.len();
		Self::can_queue_downward_message(config, &para, &msg)?;

		let inbound =
			InboundDownwardMessage { msg, sent_at: frame_system::Pallet::<T>::block_number() };

		// obtain the new link in the MQC and update the head.
		DownwardMessageQueueHeads::<T>::mutate(para, |head| {
			let new_head =
				BlakeTwo256::hash_of(&(*head, inbound.sent_at, T::Hashing::hash_of(&inbound.msg)));
			*head = new_head;
		});

		let q_len = DownwardMessageQueues::<T>::mutate(para, |v| {
			v.push(inbound);
			v.len()
		});

		let threshold =
			Self::dmq_max_length(config.max_downward_message_size).saturating_div(THRESHOLD_FACTOR);
		if q_len > (threshold as usize) {
			Self::increase_fee_factor(para, serialized_len as u128);
		}

		Ok(())
	}

	/// Checks if the number of processed downward messages is valid.
	pub(crate) fn check_processed_downward_messages(
		para: ParaId,
		relay_parent_number: BlockNumberFor<T>,
		processed_downward_messages: u32,
	) -> Result<(), ProcessedDownwardMessagesAcceptanceErr> {
		let dmq_length = Self::dmq_length(para);

		if dmq_length > 0 && processed_downward_messages == 0 {
			// The advancement rule is for at least one downwards message to be processed
			// if the queue is non-empty at the relay-parent. Downwards messages are annotated
			// with the block number, so we compare the earliest (first) against the relay parent.
			let contents = Self::dmq_contents(para);

			// sanity: if dmq_length is >0 this should always be 'Some'.
			if contents.get(0).map_or(false, |msg| msg.sent_at <= relay_parent_number) {
				return Err(ProcessedDownwardMessagesAcceptanceErr::AdvancementRule)
			}
		}

		// Note that we might be allowing a parachain to signal that it's processed
		// messages that hadn't been placed in the queue at the relay_parent.
		// only 'stupid' parachains would do it and we don't (and can't) force anyone
		// to act on messages, so the lenient approach is fine here.
		if dmq_length < processed_downward_messages {
			return Err(ProcessedDownwardMessagesAcceptanceErr::Underflow {
				processed_downward_messages,
				dmq_length,
			})
		}

		Ok(())
	}

	/// Prunes the specified number of messages from the downward message queue of the given para.
	pub(crate) fn prune_dmq(para: ParaId, processed_downward_messages: u32) {
		let q_len = DownwardMessageQueues::<T>::mutate(para, |q| {
			let processed_downward_messages = processed_downward_messages as usize;
			if processed_downward_messages > q.len() {
				// reaching this branch is unexpected due to the constraint established by
				// `check_processed_downward_messages`. But better be safe than sorry.
				q.clear();
			} else {
				*q = q.split_off(processed_downward_messages);
			}
			q.len()
		});

		let config = configuration::ActiveConfig::<T>::get();
		let threshold =
			Self::dmq_max_length(config.max_downward_message_size).saturating_div(THRESHOLD_FACTOR);
		if q_len <= (threshold as usize) {
			Self::decrease_fee_factor(para);
		}
	}

	/// Returns the Head of Message Queue Chain for the given para or `None` if there is none
	/// associated with it.
	#[cfg(test)]
	fn dmq_mqc_head(para: ParaId) -> Hash {
		DownwardMessageQueueHeads::<T>::get(&para)
	}

	/// Returns the number of pending downward messages addressed to the given para.
	///
	/// Returns 0 if the para doesn't have an associated downward message queue.
	pub(crate) fn dmq_length(para: ParaId) -> u32 {
		DownwardMessageQueues::<T>::decode_len(&para)
			.unwrap_or(0)
			.saturated_into::<u32>()
	}

	fn dmq_max_length(max_downward_message_size: u32) -> u32 {
		MAX_POSSIBLE_ALLOCATION.checked_div(max_downward_message_size).unwrap_or(0)
	}

	/// Returns the downward message queue contents for the given para.
	///
	/// The most recent messages are the latest in the vector.
	pub(crate) fn dmq_contents(
		recipient: ParaId,
	) -> Vec<InboundDownwardMessage<BlockNumberFor<T>>> {
		DownwardMessageQueues::<T>::get(&recipient)
	}

	/// Make the parachain reachable for downward messages.
	///
	/// Only useable in benchmarks or tests.
	#[cfg(any(feature = "runtime-benchmarks", feature = "std"))]
	pub fn make_parachain_reachable(para: impl Into<ParaId>) {
		let para = para.into();
		crate::paras::Heads::<T>::insert(para, para.encode());
	}
}

impl<T: Config> FeeTracker for Pallet<T> {
	type Id = ParaId;

	fn get_fee_factor(id: Self::Id) -> FixedU128 {
		DeliveryFeeFactor::<T>::get(id)
	}

	fn set_fee_factor(id: Self::Id, val: FixedU128) {
		<DeliveryFeeFactor<T>>::set(id, val);
	}
}

#[cfg(feature = "runtime-benchmarks")]
impl<T: Config> crate::EnsureForParachain for Pallet<T> {
	fn ensure(para: ParaId) {
		Self::make_parachain_reachable(para);
	}
}
