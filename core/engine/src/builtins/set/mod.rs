//! Boa's implementation of ECMAScript's global `Set` object.
//!
//! The ECMAScript `Set` class is a global object that is used in the construction of sets; which
//! are high-level, collections of values.
//!
//! More information:
//!  - [ECMAScript reference][spec]
//!  - [MDN documentation][mdn]
//!
//! [spec]: https://tc39.es/ecma262/#sec-set-objects
//! [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Set

mod set_iterator;

#[cfg(test)]
mod tests;

pub mod ordered_set;

use self::ordered_set::OrderedSet;
use crate::{
    builtins::{BuiltInBuilder, BuiltInConstructor, BuiltInObject, IntrinsicObject},
    context::intrinsics::{Intrinsics, StandardConstructor, StandardConstructors},
    error::JsNativeError,
    js_string,
    object::{internal_methods::get_prototype_from_constructor, JsObject},
    property::{Attribute, PropertyNameKind},
    realm::Realm,
    string::StaticJsStrings,
    symbol::JsSymbol,
    Context, JsArgs, JsResult, JsString, JsValue,
};
use boa_profiler::Profiler;
use num_traits::Zero;

pub(crate) use set_iterator::SetIterator;

use super::iterable::IteratorHint;

#[derive(Debug, Clone)]
pub(crate) struct Set;

impl IntrinsicObject for Set {
    fn get(intrinsics: &Intrinsics) -> JsObject {
        Self::STANDARD_CONSTRUCTOR(intrinsics.constructors()).constructor()
    }
    fn init(realm: &Realm) {
        let _timer = Profiler::global().start_event(std::any::type_name::<Self>(), "init");

        let get_species = BuiltInBuilder::callable(realm, Self::get_species)
            .name(js_string!("get [Symbol.species]"))
            .build();

        let size_getter = BuiltInBuilder::callable(realm, Self::size_getter)
            .name(js_string!("get size"))
            .build();

        let values_function = BuiltInBuilder::callable(realm, Self::values)
            .name(js_string!("values"))
            .build();

        BuiltInBuilder::from_standard_constructor::<Self>(realm)
            .static_accessor(
                JsSymbol::species(),
                Some(get_species),
                None,
                Attribute::CONFIGURABLE,
            )
            .method(Self::add, js_string!("add"), 1)
            .method(Self::clear, js_string!("clear"), 0)
            .method(Self::delete, js_string!("delete"), 1)
            .method(Self::entries, js_string!("entries"), 0)
            .method(Self::for_each, js_string!("forEach"), 1)
            .method(Self::has, js_string!("has"), 1)
            .property(
                js_string!("keys"),
                values_function.clone(),
                Attribute::WRITABLE | Attribute::NON_ENUMERABLE | Attribute::CONFIGURABLE,
            )
            .accessor(
                js_string!("size"),
                Some(size_getter),
                None,
                Attribute::CONFIGURABLE,
            )
            .property(
                js_string!("values"),
                values_function.clone(),
                Attribute::WRITABLE | Attribute::NON_ENUMERABLE | Attribute::CONFIGURABLE,
            )
            .property(
                JsSymbol::iterator(),
                values_function,
                Attribute::WRITABLE | Attribute::NON_ENUMERABLE | Attribute::CONFIGURABLE,
            )
            .property(
                JsSymbol::to_string_tag(),
                Self::NAME,
                Attribute::READONLY | Attribute::NON_ENUMERABLE | Attribute::CONFIGURABLE,
            )
            .build();
    }
}

impl BuiltInObject for Set {
    const NAME: JsString = StaticJsStrings::SET;
}

impl BuiltInConstructor for Set {
    const LENGTH: usize = 0;
    const P: usize = 11;
    const SP: usize = 1;
    const STANDARD_CONSTRUCTOR: fn(&StandardConstructors) -> &StandardConstructor =
        StandardConstructors::set;

    /// [`Set ( [ iterable ] )`][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-set-iterable
    fn constructor(
        new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        // 1. If NewTarget is undefined, throw a TypeError exception.
        if new_target.is_undefined() {
            return Err(JsNativeError::typ()
                .with_message("calling a builtin Set constructor without new is forbidden")
                .into());
        }

        // 2. Let set be ? OrdinaryCreateFromConstructor(NewTarget, "%Set.prototype%", « [[SetData]] »).
        // 3. Set set.[[SetData]] to a new empty List.
        let prototype =
            get_prototype_from_constructor(new_target, StandardConstructors::set, context)?;
        let set = JsObject::from_proto_and_data_with_shared_shape(
            context.root_shape(),
            prototype,
            OrderedSet::default(),
        );

        // 4. If iterable is either undefined or null, return set.
        let iterable = args.get_or_undefined(0);
        if iterable.is_null_or_undefined() {
            return Ok(set.into());
        }

        // 5. Let adder be ? Get(set, "add").
        let adder = set.get(js_string!("add"), context)?;

        // 6. If IsCallable(adder) is false, throw a TypeError exception.
        let adder = adder.as_callable().ok_or_else(|| {
            JsNativeError::typ().with_message("'add' of 'newTarget' is not a function")
        })?;

        // 7. Let iteratorRecord be ? GetIterator(iterable, sync).
        let mut iterator_record = iterable.clone().get_iterator(IteratorHint::Sync, context)?;

        // 8. Repeat,
        //     a. Let next be ? IteratorStepValue(iteratorRecord).
        while let Some(next) = iterator_record.step_value(context)? {
            // c. Let status be Completion(Call(adder, set, « next »)).
            if let Err(status) = adder.call(&set.clone().into(), &[next], context) {
                // d. IfAbruptCloseIterator(status, iteratorRecord).
                return iterator_record.close(Err(status), context);
            }
        }

        //     b. If next is done, return set.
        Ok(set.into())
    }
}

impl Set {
    /// Utility for constructing `Set` objects.
    pub(crate) fn set_create(prototype: Option<JsObject>, context: &mut Context) -> JsObject {
        let prototype =
            prototype.unwrap_or_else(|| context.intrinsics().constructors().set().prototype());

        JsObject::from_proto_and_data_with_shared_shape(
            context.root_shape(),
            prototype,
            OrderedSet::new(),
        )
    }

    /// Utility for constructing `Set` objects from an iterator of `JsValue`'s.
    pub(crate) fn create_set_from_list<I>(elements: I, context: &mut Context) -> JsObject
    where
        I: IntoIterator<Item = JsValue>,
    {
        // Create empty Set
        let set = Self::set_create(None, context);
        // For each element e of elements, do
        for elem in elements {
            Self::add(&set.clone().into(), &[elem], context)
                .expect("adding new element shouldn't error out");
        }

        set
    }

    /// `get Set [ @@species ]`
    ///
    /// The Set[Symbol.species] accessor property returns the Set constructor.
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///  - [MDN documentation][mdn]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-get-set-@@species
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Set/@@species
    #[allow(clippy::unnecessary_wraps)]
    fn get_species(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
        // 1. Return the this value.
        Ok(this.clone())
    }

    /// `Set.prototype.add( value )`
    ///
    /// This method adds an entry with value into the set. Returns the set object
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///  - [MDN documentation][mdn]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-set.prototype.add
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Set/add
    pub(crate) fn add(this: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
        // 1. Let S be the this value.
        // 2. Perform ? RequireInternalSlot(S, [[SetData]]).
        let Some(mut set) = this
            .as_object()
            .and_then(JsObject::downcast_mut::<OrderedSet>)
        else {
            return Err(JsNativeError::typ()
                .with_message("Method Set.prototype.add called on incompatible receiver")
                .into());
        };

        // 3. For each element e of S.[[SetData]], do
        // a. If e is not empty and SameValueZero(e, value) is true, then
        // i. Return S.
        // 4. If value is -0𝔽, set value to +0𝔽.
        let value = args.get_or_undefined(0);
        let value = match value.as_number() {
            Some(n) if n.is_zero() => &JsValue::new(0),
            _ => value,
        };

        // 5. Append value to S.[[SetData]].
        set.add(value.clone());

        Ok(this.clone())
        // 6. Return S.
    }

    /// `Set.prototype.clear( )`
    ///
    /// This method removes all entries from the set.
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///  - [MDN documentation][mdn]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-set.prototype.clear
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Set/clear
    pub(crate) fn clear(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
        let Some(mut set) = this
            .as_object()
            .and_then(JsObject::downcast_mut::<OrderedSet>)
        else {
            return Err(JsNativeError::typ()
                .with_message("'this' is not a Set")
                .into());
        };

        set.clear();

        Ok(JsValue::undefined())
    }

    /// `Set.prototype.delete( value )`
    ///
    /// This method removes the entry for the given value if it exists.
    /// Returns true if there was an element, false otherwise.
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///  - [MDN documentation][mdn]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-set.prototype.delete
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Set/delete
    pub(crate) fn delete(this: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
        // 1. Let S be the this value.
        // 2. Perform ? RequireInternalSlot(S, [[SetData]]).
        let Some(mut set) = this
            .as_object()
            .and_then(JsObject::downcast_mut::<OrderedSet>)
        else {
            return Err(JsNativeError::typ()
                .with_message("Method Set.prototype.delete called on incompatible receiver")
                .into());
        };

        let value = args.get_or_undefined(0);
        let value = match value.as_number() {
            Some(n) if n.is_zero() => &JsValue::new(0),
            _ => value,
        };

        // 3. For each element e of S.[[SetData]], do
        // a. If e is not empty and SameValueZero(e, value) is true, then
        // i. Replace the element of S.[[SetData]] whose value is e with an element whose value is empty.
        // ii. Return true.
        // 4. Return false.
        Ok(set.delete(value).into())
    }

    /// `Set.prototype.entries( )`
    ///
    /// This method returns an iterator over the entries of the set
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///  - [MDN documentation][mdn]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-set.prototype.entries
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Set/entries
    pub(crate) fn entries(
        this: &JsValue,
        _: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        let Some(lock) = this.as_object().and_then(|o| {
            o.downcast_mut::<OrderedSet>()
                .map(|mut set| set.lock(o.clone()))
        }) else {
            return Err(JsNativeError::typ()
                .with_message("Method Set.prototype.entries called on incompatible receiver")
                .into());
        };

        Ok(SetIterator::create_set_iterator(
            this.clone(),
            PropertyNameKind::KeyAndValue,
            lock,
            context,
        ))
    }

    /// `Set.prototype.forEach( callbackFn [ , thisArg ] )`
    ///
    /// This method executes the provided callback function for each value in the set
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///  - [MDN documentation][mdn]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-set.prototype.foreach
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Set/foreach
    pub(crate) fn for_each(
        this: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        // 1. Let S be the this value.
        // 2. Perform ? RequireInternalSlot(S, [[SetData]]).
        let Some(lock) = this.as_object().and_then(|o| {
            o.downcast_mut::<OrderedSet>()
                .map(|mut set| set.lock(o.clone()))
        }) else {
            return Err(JsNativeError::typ()
                .with_message("Method Set.prototype.forEach called on incompatible receiver")
                .into());
        };

        // 3. If IsCallable(callbackfn) is false, throw a TypeError exception.
        let Some(callback_fn) = args.get_or_undefined(0).as_callable() else {
            return Err(JsNativeError::typ()
                .with_message(
                    "Method Set.prototype.forEach called with non-callable callback function",
                )
                .into());
        };

        // 4. Let entries be S.[[SetData]].
        // 5. Let numEntries be the number of elements in entries.
        // 6. Let index be 0.
        let mut index = 0;

        // 7. Repeat, while index < numEntries,
        while index < Self::get_size_full(this)? {
            // a. Let e be entries[index].
            let Some(set) = this
                .as_object()
                .and_then(JsObject::downcast_ref::<OrderedSet>)
            else {
                return Err(JsNativeError::typ()
                    .with_message("Method Set.prototype.forEach called on incompatible receiver")
                    .into());
            };

            let e = set.get_index(index).cloned();
            drop(set);

            // b. Set index to index + 1.
            index += 1;

            // c. If e is not empty, then
            if let Some(e) = e {
                // i. Perform ? Call(callbackfn, thisArg, « e, e, S »).
                // ii. NOTE: The number of elements in entries may have increased during execution of callbackfn.
                // iii. Set numEntries to the number of elements in entries.
                callback_fn.call(
                    args.get_or_undefined(1),
                    &[e.clone(), e.clone(), this.clone()],
                    context,
                )?;
            }
        }

        drop(lock);

        // 8. Return undefined.
        Ok(JsValue::undefined())
    }

    /// `Map.prototype.has( key )`
    ///
    /// This method checks if the map contains an entry with the given key.
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///  - [MDN documentation][mdn]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-map.prototype.has
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Map/has
    pub(crate) fn has(this: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
        // 1. Let S be the this value.
        // 2. Perform ? RequireInternalSlot(S, [[SetData]]).
        let Some(set) = this
            .as_object()
            .and_then(JsObject::downcast_ref::<OrderedSet>)
        else {
            return Err(JsNativeError::typ()
                .with_message("Method Set.prototype.has called on incompatible receiver")
                .into());
        };

        let value = args.get_or_undefined(0);
        let value = match value.as_number() {
            Some(n) if n.is_zero() => &JsValue::new(0),
            _ => value,
        };

        // 3. For each element e of S.[[SetData]], do
        // a. If e is not empty and SameValueZero(e, value) is true, return true.
        // 4. Return false.
        Ok(set.contains(value).into())
    }

    /// `Set.prototype.values( )`
    ///
    /// This method returns an iterator over the values of the set
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///  - [MDN documentation][mdn]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-set.prototype.values
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Set/values
    pub(crate) fn values(
        this: &JsValue,
        _: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        let Some(lock) = this.as_object().and_then(|o| {
            o.downcast_mut::<OrderedSet>()
                .map(|mut set| set.lock(o.clone()))
        }) else {
            return Err(JsNativeError::typ()
                .with_message("Method Set.prototype.values called on incompatible receiver")
                .into());
        };

        Ok(SetIterator::create_set_iterator(
            this.clone(),
            PropertyNameKind::Value,
            lock,
            context,
        ))
    }

    fn size_getter(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
        Self::get_size(this).map(JsValue::from)
    }

    /// Helper function to get the size of the `Set` object.
    pub(crate) fn get_size(set: &JsValue) -> JsResult<usize> {
        set.as_object()
            .and_then(|obj| {
                obj.borrow()
                    .downcast_ref::<OrderedSet>()
                    .map(OrderedSet::len)
            })
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("'this' is not a Set")
                    .into()
            })
    }

    /// Helper function to get the full size of the `Set` object.
    pub(crate) fn get_size_full(set: &JsValue) -> JsResult<usize> {
        set.as_object()
            .and_then(|obj| {
                obj.borrow()
                    .downcast_ref::<OrderedSet>()
                    .map(OrderedSet::full_len)
            })
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("'this' is not a Set")
                    .into()
            })
    }
}
