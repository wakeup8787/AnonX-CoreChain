// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use frame_support::pallet_macros::pallet_section;

#[pallet_section]
mod storage {
	#[pallet::storage]
	pub type Value<T> = StorageValue<_, u32, ValueQuery>;

	#[pallet::storage]
	pub type Map<T> = StorageMap<_, _, u32, u32, ValueQuery>;
}
