// Copyright 2017-2020 Parity Technologies (UK) Ltd.
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

//! Generic implementation of an extrinsic that has passed the verification
//! stage.

use crate::traits::{
	self, Member, MaybeDisplay, SignedExtension, Dispatchable,
};
#[allow(deprecated)]
use crate::traits::ValidateUnsigned;
use crate::transaction_validity::TransactionValidity;
use crate::generic::ExtrinsicSignature;

/// Definition of something that the external world might want to say; its
/// existence implies that it has been checked and is good, particularly with
/// regards to the signature.
#[derive(PartialEq, Eq, Clone, sp_core::RuntimeDebug)]
pub struct CheckedExtrinsic<AccountId, Call, Extra> {
	/// Who this purports to be from and the number of extrinsics have come before
	/// from the same signer, if anyone (note this is not a signature).
	pub signed: ExtrinsicSignature<(AccountId, Extra)>,

	/// The function that should be called.
	pub function: Call,
}

impl<AccountId, Call, Extra, Origin, Info> traits::Applyable for
	CheckedExtrinsic<AccountId, Call, Extra>
where
	AccountId: Member + MaybeDisplay,
	Call: Member + Dispatchable<Origin=Origin>,
	Extra: SignedExtension<AccountId=AccountId, Call=Call, DispatchInfo=Info>,
	Origin: From<Option<AccountId>>,
	Info: Clone,
{
	type AccountId = AccountId;
	type Call = Call;
	type DispatchInfo = Info;

	fn sender(&self) -> Option<&Self::AccountId> {
//		self.signed.as_ref().map(|x| &x.0)
		match self.signed.as_ref() {
			ExtrinsicSignature::Normal((a, _)) => Some(a),
			_ => None,
		}
	}

	#[allow(deprecated)] // Allow ValidateUnsigned
	fn validate<U: ValidateUnsigned<Call = Self::Call>>(
		&self,
		info: Self::DispatchInfo,
		len: usize,
	) -> TransactionValidity {
		match self.signed {
			ExtrinsicSignature::Inherent => {
				// TODO what we do when validating an inherent?
				todo!("Inherent type")
			},
			ExtrinsicSignature::Normal((ref id, ref extra)) => {
				Extra::validate(extra, id, &self.function, info.clone(), len)
			},
			ExtrinsicSignature::Detached => {
				let valid = Extra::validate_unsigned(&self.function, info, len)?;
				let unsigned_validation = U::validate_unsigned(&self.function)?;
				Ok(valid.combine_with(unsigned_validation))
			},
		}
	}

	#[allow(deprecated)] // Allow ValidateUnsigned
	fn apply<U: ValidateUnsigned<Call=Self::Call>>(
		self,
		info: Self::DispatchInfo,
		len: usize,
	) -> crate::ApplyExtrinsicResult {
		let (maybe_who, pre) = match self.signed {
			ExtrinsicSignature::Inherent => {
				// TODO what we do when applying an inherent?
				todo!("Inherent type")
			},
			ExtrinsicSignature::Normal((id, extra)) => {
				let pre = Extra::pre_dispatch(extra, &id, &self.function, info.clone(), len)?;
				(Some(id), pre)
			},
			ExtrinsicSignature::Detached => {
				let pre = Extra::pre_dispatch_unsigned(&self.function, info.clone(), len)?;
				U::pre_dispatch(&self.function)?;
				(None, pre)
			},
		};
		let res = self.function.dispatch(Origin::from(maybe_who));
		Extra::post_dispatch(pre, info.clone(), len);
		Ok(res.map_err(Into::into))
	}
}
