use std::borrow::Cow;
use std::ops::{Bound, RangeBounds};
use std::{mem, ptr};

use crate::mdb::error::mdb_result;
use crate::mdb::ffi;
use crate::types::DecodeIgnore;
use crate::*;

/// A polymorphic database that accepts types on call methods and not at creation.
///
/// # Example: Iterate over ranges of databases entries
///
/// In this example we store numbers in big endian this way those are ordered.
/// Thanks to their bytes representation, heed is able to iterate over them
/// from the lowest to the highest.
///
/// ```
/// # use std::fs;
/// # use std::path::Path;
/// # use heed::EnvOpenOptions;
/// use heed::PolyDatabase;
/// use heed::types::*;
/// use heed::byteorder::BigEndian;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let dir = tempfile::tempdir()?;
/// # let env = EnvOpenOptions::new()
/// #     .map_size(10 * 1024 * 1024) // 10MB
/// #     .max_dbs(3000)
/// #     .open(dir.path())?;
/// type BEI64 = I64<BigEndian>;
///
/// let db: PolyDatabase = env.create_poly_database(Some("big-endian-iter"))?;
///
/// let mut wtxn = env.write_txn()?;
/// # db.clear(&mut wtxn)?;
/// db.put::<OwnedType<BEI64>, Unit>(&mut wtxn, &BEI64::new(0), &())?;
/// db.put::<OwnedType<BEI64>, Str>(&mut wtxn, &BEI64::new(35), "thirty five")?;
/// db.put::<OwnedType<BEI64>, Str>(&mut wtxn, &BEI64::new(42), "forty two")?;
/// db.put::<OwnedType<BEI64>, Unit>(&mut wtxn, &BEI64::new(68), &())?;
///
/// // you can iterate over database entries in order
/// let range = BEI64::new(35)..=BEI64::new(42);
/// let mut range = db.range::<OwnedType<BEI64>, Str, _>(&wtxn, &range)?;
/// assert_eq!(range.next().transpose()?, Some((BEI64::new(35), "thirty five")));
/// assert_eq!(range.next().transpose()?, Some((BEI64::new(42), "forty two")));
/// assert_eq!(range.next().transpose()?, None);
///
/// drop(range);
/// wtxn.commit()?;
/// # Ok(()) }
/// ```
///
/// # Example: Select ranges of entries
///
/// Heed also support ranges deletions.
/// Same configuration as above, numbers are ordered, therefore it is safe to specify
/// a range and be able to iterate over and/or delete it.
///
/// ```
/// # use std::fs;
/// # use std::path::Path;
/// # use heed::EnvOpenOptions;
/// use heed::PolyDatabase;
/// use heed::types::*;
/// use heed::byteorder::BigEndian;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let dir = tempfile::tempdir()?;
/// # let env = EnvOpenOptions::new()
/// #     .map_size(10 * 1024 * 1024) // 10MB
/// #     .max_dbs(3000)
/// #     .open(dir.path())?;
/// type BEI64 = I64<BigEndian>;
///
/// let db: PolyDatabase = env.create_poly_database(Some("big-endian-iter"))?;
///
/// let mut wtxn = env.write_txn()?;
/// # db.clear(&mut wtxn)?;
/// db.put::<OwnedType<BEI64>, Unit>(&mut wtxn, &BEI64::new(0), &())?;
/// db.put::<OwnedType<BEI64>, Str>(&mut wtxn, &BEI64::new(35), "thirty five")?;
/// db.put::<OwnedType<BEI64>, Str>(&mut wtxn, &BEI64::new(42), "forty two")?;
/// db.put::<OwnedType<BEI64>, Unit>(&mut wtxn, &BEI64::new(68), &())?;
///
/// // even delete a range of keys
/// let range = BEI64::new(35)..=BEI64::new(42);
/// let deleted = db.delete_range::<OwnedType<BEI64>, _>(&mut wtxn, &range)?;
/// assert_eq!(deleted, 2);
///
/// let rets: Result<_, _> = db.iter::<OwnedType<BEI64>, Unit>(&wtxn)?.collect();
/// let rets: Vec<(BEI64, _)> = rets?;
///
/// let expected = vec![
///     (BEI64::new(0), ()),
///     (BEI64::new(68), ()),
/// ];
///
/// assert_eq!(deleted, 2);
/// assert_eq!(rets, expected);
///
/// wtxn.commit()?;
/// # Ok(()) }
/// ```
#[derive(Copy, Clone)]
pub struct PolyDatabase {
    pub(crate) env_ident: usize,
    pub(crate) dbi: ffi::MDB_dbi,
}

impl PolyDatabase {
    pub(crate) fn new(env_ident: usize, dbi: ffi::MDB_dbi) -> PolyDatabase {
        PolyDatabase { env_ident, dbi }
    }

    /// Retrieves the value associated with a key.
    ///
    /// If the key does not exist, then `None` is returned.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// let db = env.create_poly_database(Some("get-poly-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<Str, OwnedType<i32>>(&mut wtxn, "i-am-forty-two", &42)?;
    /// db.put::<Str, OwnedType<i32>>(&mut wtxn, "i-am-twenty-seven", &27)?;
    ///
    /// let ret = db.get::<Str, OwnedType<i32>>(&wtxn, "i-am-forty-two")?;
    /// assert_eq!(ret, Some(42));
    ///
    /// let ret = db.get::<Str, OwnedType<i32>>(&wtxn, "i-am-twenty-one")?;
    /// assert_eq!(ret, None);
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn get<'a, 'txn, KC, DC>(
        &self,
        txn: &'txn RoTxn,
        key: &'a KC::EItem,
    ) -> Result<Option<DC::DItem>>
    where
        KC: BytesEncode<'a>,
        DC: BytesDecode<'txn>,
    {
        assert_eq!(self.env_ident, txn.env.env_mut_ptr() as usize);

        let key_bytes: Cow<[u8]> = KC::bytes_encode(&key).ok_or(Error::Encoding)?;

        let mut key_val = unsafe { crate::into_val(&key_bytes) };
        let mut data_val = mem::MaybeUninit::uninit();

        let result = unsafe {
            mdb_result(ffi::mdb_get(txn.txn, self.dbi, &mut key_val, data_val.as_mut_ptr()))
        };

        match result {
            Ok(()) => {
                let data = unsafe { crate::from_val(data_val.assume_init()) };
                let data = DC::bytes_decode(data).ok_or(Error::Decoding)?;
                Ok(Some(data))
            }
            Err(e) if e.not_found() => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Retrieves the key/value pair lower than the given one in this database.
    ///
    /// If the database if empty or there is no key lower than the given one,
    /// then `None` is returned.
    ///
    /// Comparisons are made by using the bytes representation of the key.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEU32 = U32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("get-lt-u32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEU32>, Unit>(&mut wtxn, &BEU32::new(27), &())?;
    /// db.put::<OwnedType<BEU32>, Unit>(&mut wtxn, &BEU32::new(42), &())?;
    /// db.put::<OwnedType<BEU32>, Unit>(&mut wtxn, &BEU32::new(43), &())?;
    ///
    /// let ret = db.get_lower_than::<OwnedType<BEU32>, Unit>(&wtxn, &BEU32::new(4404))?;
    /// assert_eq!(ret, Some((BEU32::new(43), ())));
    ///
    /// let ret = db.get_lower_than::<OwnedType<BEU32>, Unit>(&wtxn, &BEU32::new(43))?;
    /// assert_eq!(ret, Some((BEU32::new(42), ())));
    ///
    /// let ret = db.get_lower_than::<OwnedType<BEU32>, Unit>(&wtxn, &BEU32::new(27))?;
    /// assert_eq!(ret, None);
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn get_lower_than<'a, 'txn, KC, DC>(
        &self,
        txn: &'txn RoTxn,
        key: &'a KC::EItem,
    ) -> Result<Option<(KC::DItem, DC::DItem)>>
    where
        KC: BytesEncode<'a> + BytesDecode<'txn>,
        DC: BytesDecode<'txn>,
    {
        assert_eq!(self.env_ident, txn.env.env_mut_ptr() as usize);

        let mut cursor = RoCursor::new(txn, self.dbi)?;
        let key_bytes: Cow<[u8]> = KC::bytes_encode(&key).ok_or(Error::Encoding)?;
        cursor.move_on_key_greater_than_or_equal_to(&key_bytes)?;

        match cursor.move_on_prev() {
            Ok(Some((key, data))) => match (KC::bytes_decode(key), DC::bytes_decode(data)) {
                (Some(key), Some(data)) => Ok(Some((key, data))),
                (_, _) => Err(Error::Decoding),
            },
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Retrieves the key/value pair lower than or equal the given one in this database.
    ///
    /// If the database if empty or there is no key lower than or equal to the given one,
    /// then `None` is returned.
    ///
    /// Comparisons are made by using the bytes representation of the key.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEU32 = U32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("get-lte-u32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEU32>, Unit>(&mut wtxn, &BEU32::new(27), &())?;
    /// db.put::<OwnedType<BEU32>, Unit>(&mut wtxn, &BEU32::new(42), &())?;
    /// db.put::<OwnedType<BEU32>, Unit>(&mut wtxn, &BEU32::new(43), &())?;
    ///
    /// let ret = db.get_lower_than_or_equal_to::<OwnedType<BEU32>, Unit>(&wtxn, &BEU32::new(4404))?;
    /// assert_eq!(ret, Some((BEU32::new(43), ())));
    ///
    /// let ret = db.get_lower_than_or_equal_to::<OwnedType<BEU32>, Unit>(&wtxn, &BEU32::new(43))?;
    /// assert_eq!(ret, Some((BEU32::new(43), ())));
    ///
    /// let ret = db.get_lower_than_or_equal_to::<OwnedType<BEU32>, Unit>(&wtxn, &BEU32::new(26))?;
    /// assert_eq!(ret, None);
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn get_lower_than_or_equal_to<'a, 'txn, KC, DC>(
        &self,
        txn: &'txn RoTxn,
        key: &'a KC::EItem,
    ) -> Result<Option<(KC::DItem, DC::DItem)>>
    where
        KC: BytesEncode<'a> + BytesDecode<'txn>,
        DC: BytesDecode<'txn>,
    {
        assert_eq!(self.env_ident, txn.env.env_mut_ptr() as usize);

        let mut cursor = RoCursor::new(txn, self.dbi)?;
        let key_bytes: Cow<[u8]> = KC::bytes_encode(&key).ok_or(Error::Encoding)?;
        let result = match cursor.move_on_key_greater_than_or_equal_to(&key_bytes) {
            Ok(Some((key, data))) if key == &key_bytes[..] => Ok(Some((key, data))),
            Ok(_) => cursor.move_on_prev(),
            Err(e) => Err(e),
        };

        match result {
            Ok(Some((key, data))) => match (KC::bytes_decode(key), DC::bytes_decode(data)) {
                (Some(key), Some(data)) => Ok(Some((key, data))),
                (_, _) => Err(Error::Decoding),
            },
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Retrieves the key/value pair greater than the given one in this database.
    ///
    /// If the database if empty or there is no key greater than the given one,
    /// then `None` is returned.
    ///
    /// Comparisons are made by using the bytes representation of the key.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEU32 = U32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("get-lt-u32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEU32>, Unit>(&mut wtxn, &BEU32::new(27), &())?;
    /// db.put::<OwnedType<BEU32>, Unit>(&mut wtxn, &BEU32::new(42), &())?;
    /// db.put::<OwnedType<BEU32>, Unit>(&mut wtxn, &BEU32::new(43), &())?;
    ///
    /// let ret = db.get_greater_than::<OwnedType<BEU32>, Unit>(&wtxn, &BEU32::new(0))?;
    /// assert_eq!(ret, Some((BEU32::new(27), ())));
    ///
    /// let ret = db.get_greater_than::<OwnedType<BEU32>, Unit>(&wtxn, &BEU32::new(42))?;
    /// assert_eq!(ret, Some((BEU32::new(43), ())));
    ///
    /// let ret = db.get_greater_than::<OwnedType<BEU32>, Unit>(&wtxn, &BEU32::new(43))?;
    /// assert_eq!(ret, None);
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn get_greater_than<'a, 'txn, KC, DC>(
        &self,
        txn: &'txn RoTxn,
        key: &'a KC::EItem,
    ) -> Result<Option<(KC::DItem, DC::DItem)>>
    where
        KC: BytesEncode<'a> + BytesDecode<'txn>,
        DC: BytesDecode<'txn>,
    {
        assert_eq!(self.env_ident, txn.env.env_mut_ptr() as usize);

        let mut cursor = RoCursor::new(txn, self.dbi)?;
        let key_bytes: Cow<[u8]> = KC::bytes_encode(&key).ok_or(Error::Encoding)?;
        let entry = match cursor.move_on_key_greater_than_or_equal_to(&key_bytes)? {
            Some((key, data)) if key > &key_bytes[..] => Some((key, data)),
            Some((_key, _data)) => cursor.move_on_next()?,
            None => None,
        };

        match entry {
            Some((key, data)) => match (KC::bytes_decode(key), DC::bytes_decode(data)) {
                (Some(key), Some(data)) => Ok(Some((key, data))),
                (_, _) => Err(Error::Decoding),
            },
            None => Ok(None),
        }
    }

    /// Retrieves the key/value pair greater than or equal the given one in this database.
    ///
    /// If the database if empty or there is no key greater than or equal to the given one,
    /// then `None` is returned.
    ///
    /// Comparisons are made by using the bytes representation of the key.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEU32 = U32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("get-lt-u32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEU32>, Unit>(&mut wtxn, &BEU32::new(27), &())?;
    /// db.put::<OwnedType<BEU32>, Unit>(&mut wtxn, &BEU32::new(42), &())?;
    /// db.put::<OwnedType<BEU32>, Unit>(&mut wtxn, &BEU32::new(43), &())?;
    ///
    /// let ret = db.get_greater_than_or_equal_to::<OwnedType<BEU32>, Unit>(&wtxn, &BEU32::new(0))?;
    /// assert_eq!(ret, Some((BEU32::new(27), ())));
    ///
    /// let ret = db.get_greater_than_or_equal_to::<OwnedType<BEU32>, Unit>(&wtxn, &BEU32::new(42))?;
    /// assert_eq!(ret, Some((BEU32::new(42), ())));
    ///
    /// let ret = db.get_greater_than_or_equal_to::<OwnedType<BEU32>, Unit>(&wtxn, &BEU32::new(44))?;
    /// assert_eq!(ret, None);
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn get_greater_than_or_equal_to<'a, 'txn, KC, DC>(
        &self,
        txn: &'txn RoTxn,
        key: &'a KC::EItem,
    ) -> Result<Option<(KC::DItem, DC::DItem)>>
    where
        KC: BytesEncode<'a> + BytesDecode<'txn>,
        DC: BytesDecode<'txn>,
    {
        assert_eq!(self.env_ident, txn.env.env_mut_ptr() as usize);

        let mut cursor = RoCursor::new(txn, self.dbi)?;
        let key_bytes: Cow<[u8]> = KC::bytes_encode(&key).ok_or(Error::Encoding)?;
        match cursor.move_on_key_greater_than_or_equal_to(&key_bytes) {
            Ok(Some((key, data))) => match (KC::bytes_decode(key), DC::bytes_decode(data)) {
                (Some(key), Some(data)) => Ok(Some((key, data))),
                (_, _) => Err(Error::Decoding),
            },
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Retrieves the first key/value pair of this database.
    ///
    /// If the database if empty, then `None` is returned.
    ///
    /// Comparisons are made by using the bytes representation of the key.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("first-poly-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    ///
    /// let ret = db.first::<OwnedType<BEI32>, Str>(&wtxn)?;
    /// assert_eq!(ret, Some((BEI32::new(27), "i-am-twenty-seven")));
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn first<'txn, KC, DC>(&self, txn: &'txn RoTxn) -> Result<Option<(KC::DItem, DC::DItem)>>
    where
        KC: BytesDecode<'txn>,
        DC: BytesDecode<'txn>,
    {
        assert_eq!(self.env_ident, txn.env.env_mut_ptr() as usize);

        let mut cursor = RoCursor::new(txn, self.dbi)?;
        match cursor.move_on_first() {
            Ok(Some((key, data))) => match (KC::bytes_decode(key), DC::bytes_decode(data)) {
                (Some(key), Some(data)) => Ok(Some((key, data))),
                (_, _) => Err(Error::Decoding),
            },
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Retrieves the last key/value pair of this database.
    ///
    /// If the database if empty, then `None` is returned.
    ///
    /// Comparisons are made by using the bytes representation of the key.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("last-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    ///
    /// let ret = db.last::<OwnedType<BEI32>, Str>(&wtxn)?;
    /// assert_eq!(ret, Some((BEI32::new(42), "i-am-forty-two")));
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn last<'txn, KC, DC>(&self, txn: &'txn RoTxn) -> Result<Option<(KC::DItem, DC::DItem)>>
    where
        KC: BytesDecode<'txn>,
        DC: BytesDecode<'txn>,
    {
        assert_eq!(self.env_ident, txn.env.env_mut_ptr() as usize);

        let mut cursor = RoCursor::new(txn, self.dbi)?;
        match cursor.move_on_last() {
            Ok(Some((key, data))) => match (KC::bytes_decode(key), DC::bytes_decode(data)) {
                (Some(key), Some(data)) => Ok(Some((key, data))),
                (_, _) => Err(Error::Decoding),
            },
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Returns the number of elements in this database.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(13), "i-am-thirteen")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(521), "i-am-five-hundred-and-twenty-one")?;
    ///
    /// let ret = db.len(&wtxn)?;
    /// assert_eq!(ret, 4);
    ///
    /// db.delete::<OwnedType<BEI32>>(&mut wtxn, &BEI32::new(27))?;
    ///
    /// let ret = db.len(&wtxn)?;
    /// assert_eq!(ret, 3);
    ///
    /// wtxn.commit()?;
    ///
    /// # Ok(()) }
    /// ```
    pub fn len<'txn>(&self, txn: &'txn RoTxn) -> Result<u64> {
        assert_eq!(self.env_ident, txn.env.env_mut_ptr() as usize);

        let mut db_stat = mem::MaybeUninit::uninit();
        let result = unsafe { mdb_result(ffi::mdb_stat(txn.txn, self.dbi, db_stat.as_mut_ptr())) };

        match result {
            Ok(()) => {
                let stats = unsafe { db_stat.assume_init() };
                Ok(stats.ms_entries as u64)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Returns `true` if and only if this database is empty.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(13), "i-am-thirteen")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(521), "i-am-five-hundred-and-twenty-one")?;
    ///
    /// let ret = db.is_empty(&wtxn)?;
    /// assert_eq!(ret, false);
    ///
    /// db.clear(&mut wtxn)?;
    ///
    /// let ret = db.is_empty(&wtxn)?;
    /// assert_eq!(ret, true);
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn is_empty<'txn>(&self, txn: &'txn RoTxn) -> Result<bool> {
        assert_eq!(self.env_ident, txn.env.env_mut_ptr() as usize);

        let mut cursor = RoCursor::new(txn, self.dbi)?;
        match cursor.move_on_first()? {
            Some(_) => Ok(false),
            None => Ok(true),
        }
    }

    /// Return a lexicographically ordered iterator of all key-value pairs in this database.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(13), "i-am-thirteen")?;
    ///
    /// let mut iter = db.iter::<OwnedType<BEI32>, Str>(&wtxn)?;
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(13), "i-am-thirteen")));
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(27), "i-am-twenty-seven")));
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(42), "i-am-forty-two")));
    /// assert_eq!(iter.next().transpose()?, None);
    ///
    /// drop(iter);
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn iter<'txn, KC, DC>(&self, txn: &'txn RoTxn) -> Result<RoIter<'txn, KC, DC>> {
        assert_eq!(self.env_ident, txn.env.env_mut_ptr() as usize);

        RoCursor::new(txn, self.dbi).map(|cursor| RoIter::new(cursor))
    }

    /// Return a mutable lexicographically ordered iterator of all key-value pairs in this database.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(13), "i-am-thirteen")?;
    ///
    /// let mut iter = db.iter_mut::<OwnedType<BEI32>, Str>(&mut wtxn)?;
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(13), "i-am-thirteen")));
    /// let ret = unsafe { iter.del_current()? };
    /// assert!(ret);
    ///
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(27), "i-am-twenty-seven")));
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(42), "i-am-forty-two")));
    /// let ret = unsafe { iter.put_current(&BEI32::new(42), "i-am-the-new-forty-two")? };
    /// assert!(ret);
    ///
    /// assert_eq!(iter.next().transpose()?, None);
    ///
    /// drop(iter);
    ///
    /// let ret = db.get::<OwnedType<BEI32>, Str>(&wtxn, &BEI32::new(13))?;
    /// assert_eq!(ret, None);
    ///
    /// let ret = db.get::<OwnedType<BEI32>, Str>(&wtxn, &BEI32::new(42))?;
    /// assert_eq!(ret, Some("i-am-the-new-forty-two"));
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn iter_mut<'txn, KC, DC>(&self, txn: &'txn mut RwTxn) -> Result<RwIter<'txn, KC, DC>> {
        assert_eq!(self.env_ident, txn.txn.env.env_mut_ptr() as usize);

        RwCursor::new(txn, self.dbi).map(|cursor| RwIter::new(cursor))
    }

    /// Returns a reversed lexicographically ordered iterator of all key-value pairs in this database.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(13), "i-am-thirteen")?;
    ///
    /// let mut iter = db.rev_iter::<OwnedType<BEI32>, Str>(&wtxn)?;
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(42), "i-am-forty-two")));
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(27), "i-am-twenty-seven")));
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(13), "i-am-thirteen")));
    /// assert_eq!(iter.next().transpose()?, None);
    ///
    /// drop(iter);
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn rev_iter<'txn, KC, DC>(&self, txn: &'txn RoTxn) -> Result<RoRevIter<'txn, KC, DC>> {
        assert_eq!(self.env_ident, txn.env.env_mut_ptr() as usize);

        RoCursor::new(txn, self.dbi).map(|cursor| RoRevIter::new(cursor))
    }

    /// Return a mutable reversed lexicographically ordered iterator of all key-value pairs
    /// in this database.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(13), "i-am-thirteen")?;
    ///
    /// let mut iter = db.rev_iter_mut::<OwnedType<BEI32>, Str>(&mut wtxn)?;
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(42), "i-am-forty-two")));
    /// let ret = unsafe { iter.del_current()? };
    /// assert!(ret);
    ///
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(27), "i-am-twenty-seven")));
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(13), "i-am-thirteen")));
    /// let ret = unsafe { iter.put_current(&BEI32::new(13), "i-am-the-new-thirteen")? };
    /// assert!(ret);
    ///
    /// assert_eq!(iter.next().transpose()?, None);
    ///
    /// drop(iter);
    ///
    /// let ret = db.get::<OwnedType<BEI32>, Str>(&wtxn, &BEI32::new(42))?;
    /// assert_eq!(ret, None);
    ///
    /// let ret = db.get::<OwnedType<BEI32>, Str>(&wtxn, &BEI32::new(13))?;
    /// assert_eq!(ret, Some("i-am-the-new-thirteen"));
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn rev_iter_mut<'txn, KC, DC>(
        &self,
        txn: &'txn mut RwTxn,
    ) -> Result<RwRevIter<'txn, KC, DC>> {
        assert_eq!(self.env_ident, txn.env.env_mut_ptr() as usize);

        RwCursor::new(txn, self.dbi).map(|cursor| RwRevIter::new(cursor))
    }

    /// Return a lexicographically ordered iterator of a range of key-value pairs in this database.
    ///
    /// Comparisons are made by using the bytes representation of the key.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(13), "i-am-thirteen")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(521), "i-am-five-hundred-and-twenty-one")?;
    ///
    /// let range = BEI32::new(27)..=BEI32::new(42);
    /// let mut iter = db.range::<OwnedType<BEI32>, Str, _>(&wtxn, &range)?;
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(27), "i-am-twenty-seven")));
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(42), "i-am-forty-two")));
    /// assert_eq!(iter.next().transpose()?, None);
    ///
    /// drop(iter);
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn range<'a, 'txn, KC, DC, R>(
        &self,
        txn: &'txn RoTxn,
        range: &'a R,
    ) -> Result<RoRange<'txn, KC, DC>>
    where
        KC: BytesEncode<'a>,
        R: RangeBounds<KC::EItem>,
    {
        assert_eq!(self.env_ident, txn.env.env_mut_ptr() as usize);

        let start_bound = match range.start_bound() {
            Bound::Included(bound) => {
                let bytes = KC::bytes_encode(bound).ok_or(Error::Encoding)?;
                Bound::Included(bytes.into_owned())
            }
            Bound::Excluded(bound) => {
                let bytes = KC::bytes_encode(bound).ok_or(Error::Encoding)?;
                Bound::Excluded(bytes.into_owned())
            }
            Bound::Unbounded => Bound::Unbounded,
        };

        let end_bound = match range.end_bound() {
            Bound::Included(bound) => {
                let bytes = KC::bytes_encode(bound).ok_or(Error::Encoding)?;
                Bound::Included(bytes.into_owned())
            }
            Bound::Excluded(bound) => {
                let bytes = KC::bytes_encode(bound).ok_or(Error::Encoding)?;
                Bound::Excluded(bytes.into_owned())
            }
            Bound::Unbounded => Bound::Unbounded,
        };

        RoCursor::new(txn, self.dbi).map(|cursor| RoRange::new(cursor, start_bound, end_bound))
    }

    /// Return a mutable lexicographically ordered iterator of a range of
    /// key-value pairs in this database.
    ///
    /// Comparisons are made by using the bytes representation of the key.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(13), "i-am-thirteen")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(521), "i-am-five-hundred-and-twenty-one")?;
    ///
    /// let range = BEI32::new(27)..=BEI32::new(42);
    /// let mut range = db.range_mut::<OwnedType<BEI32>, Str, _>(&mut wtxn, &range)?;
    /// assert_eq!(range.next().transpose()?, Some((BEI32::new(27), "i-am-twenty-seven")));
    /// let ret = unsafe { range.del_current()? };
    /// assert!(ret);
    /// assert_eq!(range.next().transpose()?, Some((BEI32::new(42), "i-am-forty-two")));
    /// let ret = unsafe { range.put_current(&BEI32::new(42), "i-am-the-new-forty-two")? };
    /// assert!(ret);
    ///
    /// assert_eq!(range.next().transpose()?, None);
    /// drop(range);
    ///
    ///
    /// let mut iter = db.iter::<OwnedType<BEI32>, Str>(&wtxn)?;
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(13), "i-am-thirteen")));
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(42), "i-am-the-new-forty-two")));
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(521), "i-am-five-hundred-and-twenty-one")));
    /// assert_eq!(iter.next().transpose()?, None);
    ///
    /// drop(iter);
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn range_mut<'a, 'txn, KC, DC, R>(
        &self,
        txn: &'txn mut RwTxn,
        range: &'a R,
    ) -> Result<RwRange<'txn, KC, DC>>
    where
        KC: BytesEncode<'a>,
        R: RangeBounds<KC::EItem>,
    {
        assert_eq!(self.env_ident, txn.txn.env.env_mut_ptr() as usize);

        let start_bound = match range.start_bound() {
            Bound::Included(bound) => {
                let bytes = KC::bytes_encode(bound).ok_or(Error::Encoding)?;
                Bound::Included(bytes.into_owned())
            }
            Bound::Excluded(bound) => {
                let bytes = KC::bytes_encode(bound).ok_or(Error::Encoding)?;
                Bound::Excluded(bytes.into_owned())
            }
            Bound::Unbounded => Bound::Unbounded,
        };

        let end_bound = match range.end_bound() {
            Bound::Included(bound) => {
                let bytes = KC::bytes_encode(bound).ok_or(Error::Encoding)?;
                Bound::Included(bytes.into_owned())
            }
            Bound::Excluded(bound) => {
                let bytes = KC::bytes_encode(bound).ok_or(Error::Encoding)?;
                Bound::Excluded(bytes.into_owned())
            }
            Bound::Unbounded => Bound::Unbounded,
        };

        RwCursor::new(txn, self.dbi).map(|cursor| RwRange::new(cursor, start_bound, end_bound))
    }

    /// Return a reversed lexicographically ordered iterator of a range of key-value
    /// pairs in this database.
    ///
    /// Comparisons are made by using the bytes representation of the key.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(13), "i-am-thirteen")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(521), "i-am-five-hundred-and-twenty-one")?;
    ///
    /// let range = BEI32::new(27)..=BEI32::new(43);
    /// let mut iter = db.rev_range::<OwnedType<BEI32>, Str, _>(&wtxn, &range)?;
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(42), "i-am-forty-two")));
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(27), "i-am-twenty-seven")));
    /// assert_eq!(iter.next().transpose()?, None);
    ///
    /// drop(iter);
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn rev_range<'a, 'txn, KC, DC, R>(
        &self,
        txn: &'txn RoTxn,
        range: &'a R,
    ) -> Result<RoRevRange<'txn, KC, DC>>
    where
        KC: BytesEncode<'a>,
        R: RangeBounds<KC::EItem>,
    {
        assert_eq!(self.env_ident, txn.env.env_mut_ptr() as usize);

        let start_bound = match range.start_bound() {
            Bound::Included(bound) => {
                let bytes = KC::bytes_encode(bound).ok_or(Error::Encoding)?;
                Bound::Included(bytes.into_owned())
            }
            Bound::Excluded(bound) => {
                let bytes = KC::bytes_encode(bound).ok_or(Error::Encoding)?;
                Bound::Excluded(bytes.into_owned())
            }
            Bound::Unbounded => Bound::Unbounded,
        };

        let end_bound = match range.end_bound() {
            Bound::Included(bound) => {
                let bytes = KC::bytes_encode(bound).ok_or(Error::Encoding)?;
                Bound::Included(bytes.into_owned())
            }
            Bound::Excluded(bound) => {
                let bytes = KC::bytes_encode(bound).ok_or(Error::Encoding)?;
                Bound::Excluded(bytes.into_owned())
            }
            Bound::Unbounded => Bound::Unbounded,
        };

        RoCursor::new(txn, self.dbi).map(|cursor| RoRevRange::new(cursor, start_bound, end_bound))
    }

    /// Return a mutable reversed lexicographically ordered iterator of a range of
    /// key-value pairs in this database.
    ///
    /// Comparisons are made by using the bytes representation of the key.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(13), "i-am-thirteen")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(521), "i-am-five-hundred-and-twenty-one")?;
    ///
    /// let range = BEI32::new(27)..=BEI32::new(42);
    /// let mut range = db.rev_range_mut::<OwnedType<BEI32>, Str, _>(&mut wtxn, &range)?;
    /// assert_eq!(range.next().transpose()?, Some((BEI32::new(42), "i-am-forty-two")));
    /// let ret = unsafe { range.del_current()? };
    /// assert!(ret);
    /// assert_eq!(range.next().transpose()?, Some((BEI32::new(27), "i-am-twenty-seven")));
    /// let ret = unsafe { range.put_current(&BEI32::new(27), "i-am-the-new-twenty-seven")? };
    /// assert!(ret);
    ///
    /// assert_eq!(range.next().transpose()?, None);
    /// drop(range);
    ///
    ///
    /// let mut iter = db.iter::<OwnedType<BEI32>, Str>(&wtxn)?;
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(13), "i-am-thirteen")));
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(27), "i-am-the-new-twenty-seven")));
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(521), "i-am-five-hundred-and-twenty-one")));
    /// assert_eq!(iter.next().transpose()?, None);
    ///
    /// drop(iter);
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn rev_range_mut<'a, 'txn, KC, DC, R>(
        &self,
        txn: &'txn mut RwTxn,
        range: &'a R,
    ) -> Result<RwRevRange<'txn, KC, DC>>
    where
        KC: BytesEncode<'a>,
        R: RangeBounds<KC::EItem>,
    {
        assert_eq!(self.env_ident, txn.txn.env.env_mut_ptr() as usize);

        let start_bound = match range.start_bound() {
            Bound::Included(bound) => {
                let bytes = KC::bytes_encode(bound).ok_or(Error::Encoding)?;
                Bound::Included(bytes.into_owned())
            }
            Bound::Excluded(bound) => {
                let bytes = KC::bytes_encode(bound).ok_or(Error::Encoding)?;
                Bound::Excluded(bytes.into_owned())
            }
            Bound::Unbounded => Bound::Unbounded,
        };

        let end_bound = match range.end_bound() {
            Bound::Included(bound) => {
                let bytes = KC::bytes_encode(bound).ok_or(Error::Encoding)?;
                Bound::Included(bytes.into_owned())
            }
            Bound::Excluded(bound) => {
                let bytes = KC::bytes_encode(bound).ok_or(Error::Encoding)?;
                Bound::Excluded(bytes.into_owned())
            }
            Bound::Unbounded => Bound::Unbounded,
        };

        RwCursor::new(txn, self.dbi).map(|cursor| RwRevRange::new(cursor, start_bound, end_bound))
    }

    /// Return a lexicographically ordered iterator of all key-value pairs
    /// in this database that starts with the given prefix.
    ///
    /// Comparisons are made by using the bytes representation of the key.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-twenty-eight", &BEI32::new(28))?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-twenty-seven", &BEI32::new(27))?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-twenty-nine",  &BEI32::new(29))?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-forty-one",    &BEI32::new(41))?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-forty-two",    &BEI32::new(42))?;
    ///
    /// let mut iter = db.prefix_iter::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-twenty")?;
    /// assert_eq!(iter.next().transpose()?, Some(("i-am-twenty-eight", BEI32::new(28))));
    /// assert_eq!(iter.next().transpose()?, Some(("i-am-twenty-nine", BEI32::new(29))));
    /// assert_eq!(iter.next().transpose()?, Some(("i-am-twenty-seven", BEI32::new(27))));
    /// assert_eq!(iter.next().transpose()?, None);
    ///
    /// drop(iter);
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn prefix_iter<'a, 'txn, KC, DC>(
        &self,
        txn: &'txn RoTxn,
        prefix: &'a KC::EItem,
    ) -> Result<RoPrefix<'txn, KC, DC>>
    where
        KC: BytesEncode<'a>,
    {
        assert_eq!(self.env_ident, txn.env.env_mut_ptr() as usize);

        let prefix_bytes = KC::bytes_encode(prefix).ok_or(Error::Encoding)?;
        let prefix_bytes = prefix_bytes.into_owned();
        RoCursor::new(txn, self.dbi).map(|cursor| RoPrefix::new(cursor, prefix_bytes))
    }

    /// Return a mutable lexicographically ordered iterator of all key-value pairs
    /// in this database that starts with the given prefix.
    ///
    /// Comparisons are made by using the bytes representation of the key.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-twenty-eight", &BEI32::new(28))?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-twenty-seven", &BEI32::new(27))?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-twenty-nine",  &BEI32::new(29))?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-forty-one",    &BEI32::new(41))?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-forty-two",    &BEI32::new(42))?;
    ///
    /// let mut iter = db.prefix_iter_mut::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-twenty")?;
    /// assert_eq!(iter.next().transpose()?, Some(("i-am-twenty-eight", BEI32::new(28))));
    /// let ret = unsafe { iter.del_current()? };
    /// assert!(ret);
    ///
    /// assert_eq!(iter.next().transpose()?, Some(("i-am-twenty-nine", BEI32::new(29))));
    /// assert_eq!(iter.next().transpose()?, Some(("i-am-twenty-seven", BEI32::new(27))));
    /// let ret = unsafe { iter.put_current("i-am-twenty-seven", &BEI32::new(27000))? };
    /// assert!(ret);
    ///
    /// assert_eq!(iter.next().transpose()?, None);
    ///
    /// drop(iter);
    ///
    /// let ret = db.get::<Str, OwnedType<BEI32>>(&wtxn, "i-am-twenty-eight")?;
    /// assert_eq!(ret, None);
    ///
    /// let ret = db.get::<Str, OwnedType<BEI32>>(&wtxn, "i-am-twenty-seven")?;
    /// assert_eq!(ret, Some(BEI32::new(27000)));
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn prefix_iter_mut<'a, 'txn, KC, DC>(
        &self,
        txn: &'txn mut RwTxn,
        prefix: &'a KC::EItem,
    ) -> Result<RwPrefix<'txn, KC, DC>>
    where
        KC: BytesEncode<'a>,
    {
        assert_eq!(self.env_ident, txn.txn.env.env_mut_ptr() as usize);

        let prefix_bytes = KC::bytes_encode(prefix).ok_or(Error::Encoding)?;
        let prefix_bytes = prefix_bytes.into_owned();
        RwCursor::new(txn, self.dbi).map(|cursor| RwPrefix::new(cursor, prefix_bytes))
    }

    /// Return a reversed lexicographically ordered iterator of all key-value pairs
    /// in this database that starts with the given prefix.
    ///
    /// Comparisons are made by using the bytes representation of the key.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-twenty-eight", &BEI32::new(28))?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-twenty-seven", &BEI32::new(27))?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-twenty-nine",  &BEI32::new(29))?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-forty-one",    &BEI32::new(41))?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-forty-two",    &BEI32::new(42))?;
    ///
    /// let mut iter = db.rev_prefix_iter::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-twenty")?;
    /// assert_eq!(iter.next().transpose()?, Some(("i-am-twenty-seven", BEI32::new(27))));
    /// assert_eq!(iter.next().transpose()?, Some(("i-am-twenty-nine", BEI32::new(29))));
    /// assert_eq!(iter.next().transpose()?, Some(("i-am-twenty-eight", BEI32::new(28))));
    /// assert_eq!(iter.next().transpose()?, None);
    ///
    /// drop(iter);
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn rev_prefix_iter<'a, 'txn, KC, DC>(
        &self,
        txn: &'txn RoTxn,
        prefix: &'a KC::EItem,
    ) -> Result<RoRevPrefix<'txn, KC, DC>>
    where
        KC: BytesEncode<'a>,
    {
        assert_eq!(self.env_ident, txn.env.env_mut_ptr() as usize);

        let prefix_bytes = KC::bytes_encode(prefix).ok_or(Error::Encoding)?;
        let prefix_bytes = prefix_bytes.into_owned();
        RoCursor::new(txn, self.dbi).map(|cursor| RoRevPrefix::new(cursor, prefix_bytes))
    }

    /// Return a mutable lexicographically ordered iterator of all key-value pairs
    /// in this database that starts with the given prefix.
    ///
    /// Comparisons are made by using the bytes representation of the key.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-twenty-eight", &BEI32::new(28))?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-twenty-seven", &BEI32::new(27))?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-twenty-nine",  &BEI32::new(29))?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-forty-one",    &BEI32::new(41))?;
    /// db.put::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-forty-two",    &BEI32::new(42))?;
    ///
    /// let mut iter = db.rev_prefix_iter_mut::<Str, OwnedType<BEI32>>(&mut wtxn, "i-am-twenty")?;
    /// assert_eq!(iter.next().transpose()?, Some(("i-am-twenty-seven", BEI32::new(27))));
    /// let ret = unsafe { iter.del_current()? };
    /// assert!(ret);
    ///
    /// assert_eq!(iter.next().transpose()?, Some(("i-am-twenty-nine", BEI32::new(29))));
    /// assert_eq!(iter.next().transpose()?, Some(("i-am-twenty-eight", BEI32::new(28))));
    /// let ret = unsafe { iter.put_current("i-am-twenty-eight", &BEI32::new(28000))? };
    /// assert!(ret);
    ///
    /// assert_eq!(iter.next().transpose()?, None);
    ///
    /// drop(iter);
    ///
    /// let ret = db.get::<Str, OwnedType<BEI32>>(&wtxn, "i-am-twenty-seven")?;
    /// assert_eq!(ret, None);
    ///
    /// let ret = db.get::<Str, OwnedType<BEI32>>(&wtxn, "i-am-twenty-eight")?;
    /// assert_eq!(ret, Some(BEI32::new(28000)));
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn rev_prefix_iter_mut<'a, 'txn, KC, DC>(
        &self,
        txn: &'txn mut RwTxn,
        prefix: &'a KC::EItem,
    ) -> Result<RwRevPrefix<'txn, KC, DC>>
    where
        KC: BytesEncode<'a>,
    {
        assert_eq!(self.env_ident, txn.txn.env.env_mut_ptr() as usize);

        let prefix_bytes = KC::bytes_encode(prefix).ok_or(Error::Encoding)?;
        let prefix_bytes = prefix_bytes.into_owned();
        RwCursor::new(txn, self.dbi).map(|cursor| RwRevPrefix::new(cursor, prefix_bytes))
    }

    /// Insert a key-value pairs in this database.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(13), "i-am-thirteen")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(521), "i-am-five-hundred-and-twenty-one")?;
    ///
    /// let ret = db.get::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27))?;
    /// assert_eq!(ret, Some("i-am-twenty-seven"));
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn put<'a, KC, DC>(
        &self,
        txn: &mut RwTxn,
        key: &'a KC::EItem,
        data: &'a DC::EItem,
    ) -> Result<()>
    where
        KC: BytesEncode<'a>,
        DC: BytesEncode<'a>,
    {
        assert_eq!(self.env_ident, txn.txn.env.env_mut_ptr() as usize);

        let key_bytes: Cow<[u8]> = KC::bytes_encode(&key).ok_or(Error::Encoding)?;
        let data_bytes: Cow<[u8]> = DC::bytes_encode(&data).ok_or(Error::Encoding)?;

        let mut key_val = unsafe { crate::into_val(&key_bytes) };
        let mut data_val = unsafe { crate::into_val(&data_bytes) };
        let flags = 0;

        unsafe {
            mdb_result(ffi::mdb_put(txn.txn.txn, self.dbi, &mut key_val, &mut data_val, flags))?
        }

        Ok(())
    }

    /// Append the given key/data pair to the end of the database.
    ///
    /// This option allows fast bulk loading when keys are already known to be in the correct order.
    /// Loading unsorted keys will cause a MDB_KEYEXIST error.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("append-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(13), "i-am-thirteen")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(521), "i-am-five-hundred-and-twenty-one")?;
    ///
    /// let ret = db.get::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27))?;
    /// assert_eq!(ret, Some("i-am-twenty-seven"));
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn append<'a, KC, DC>(
        &self,
        txn: &mut RwTxn,
        key: &'a KC::EItem,
        data: &'a DC::EItem,
    ) -> Result<()>
    where
        KC: BytesEncode<'a>,
        DC: BytesEncode<'a>,
    {
        assert_eq!(self.env_ident, txn.txn.env.env_mut_ptr() as usize);

        let key_bytes: Cow<[u8]> = KC::bytes_encode(&key).ok_or(Error::Encoding)?;
        let data_bytes: Cow<[u8]> = DC::bytes_encode(&data).ok_or(Error::Encoding)?;

        let mut key_val = unsafe { crate::into_val(&key_bytes) };
        let mut data_val = unsafe { crate::into_val(&data_bytes) };
        let flags = ffi::MDB_APPEND;

        unsafe {
            mdb_result(ffi::mdb_put(txn.txn.txn, self.dbi, &mut key_val, &mut data_val, flags))?
        }

        Ok(())
    }

    /// Deletes a key-value pairs in this database.
    ///
    /// If the key does not exist, then `false` is returned.
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(13), "i-am-thirteen")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(521), "i-am-five-hundred-and-twenty-one")?;
    ///
    /// let ret = db.delete::<OwnedType<BEI32>>(&mut wtxn, &BEI32::new(27))?;
    /// assert_eq!(ret, true);
    ///
    /// let ret = db.get::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27))?;
    /// assert_eq!(ret, None);
    ///
    /// let ret = db.delete::<OwnedType<BEI32>>(&mut wtxn, &BEI32::new(467))?;
    /// assert_eq!(ret, false);
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn delete<'a, KC>(&self, txn: &mut RwTxn, key: &'a KC::EItem) -> Result<bool>
    where
        KC: BytesEncode<'a>,
    {
        assert_eq!(self.env_ident, txn.txn.env.env_mut_ptr() as usize);

        let key_bytes: Cow<[u8]> = KC::bytes_encode(&key).ok_or(Error::Encoding)?;
        let mut key_val = unsafe { crate::into_val(&key_bytes) };

        let result = unsafe {
            mdb_result(ffi::mdb_del(txn.txn.txn, self.dbi, &mut key_val, ptr::null_mut()))
        };

        match result {
            Ok(()) => Ok(true),
            Err(e) if e.not_found() => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Deletes a range of key-value pairs in this database.
    ///
    /// Perfer using [`clear`] instead of a call to this method with a full range ([`..`]).
    ///
    /// Comparisons are made by using the bytes representation of the key.
    ///
    /// [`clear`]: crate::Database::clear
    /// [`..`]: std::ops::RangeFull
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(13), "i-am-thirteen")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(521), "i-am-five-hundred-and-twenty-one")?;
    ///
    /// let range = BEI32::new(27)..=BEI32::new(42);
    /// let ret = db.delete_range::<OwnedType<BEI32>, _>(&mut wtxn, &range)?;
    /// assert_eq!(ret, 2);
    ///
    /// let mut iter = db.iter::<OwnedType<BEI32>, Str>(&wtxn)?;
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(13), "i-am-thirteen")));
    /// assert_eq!(iter.next().transpose()?, Some((BEI32::new(521), "i-am-five-hundred-and-twenty-one")));
    /// assert_eq!(iter.next().transpose()?, None);
    ///
    /// drop(iter);
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn delete_range<'a, 'txn, KC, R>(&self, txn: &'txn mut RwTxn, range: &'a R) -> Result<usize>
    where
        KC: BytesEncode<'a> + BytesDecode<'txn>,
        R: RangeBounds<KC::EItem>,
    {
        assert_eq!(self.env_ident, txn.txn.env.env_mut_ptr() as usize);

        let mut count = 0;
        let mut iter = self.range_mut::<KC, DecodeIgnore, _>(txn, range)?;

        while iter.next().is_some() {
            // safety: We do not keep any reference from the database while using `del_current`.
            //         The user can't keep any reference inside of the database as we ask for a
            //         mutable reference to the `txn`.
            unsafe { iter.del_current()? };
            count += 1;
        }

        Ok(count)
    }

    /// Deletes all key/value pairs in this database.
    ///
    /// Perfer using this method instead of a call to [`delete_range`] with a full range ([`..`]).
    ///
    /// [`delete_range`]: crate::Database::delete_range
    /// [`..`]: std::ops::RangeFull
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::Database;
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(13), "i-am-thirteen")?;
    /// db.put::<OwnedType<BEI32>, Str>(&mut wtxn, &BEI32::new(521), "i-am-five-hundred-and-twenty-one")?;
    ///
    /// db.clear(&mut wtxn)?;
    ///
    /// let ret = db.is_empty(&wtxn)?;
    /// assert!(ret);
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn clear(&self, txn: &mut RwTxn) -> Result<()> {
        assert_eq!(self.env_ident, txn.txn.env.env_mut_ptr() as usize);

        unsafe { mdb_result(ffi::mdb_drop(txn.txn.txn, self.dbi, 0)).map_err(Into::into) }
    }

    /// Read this polymorphic database like a typed one, specifying the codecs.
    ///
    /// # Safety
    ///
    /// It is up to you to ensure that the data read and written using the polymorphic
    /// handle correspond to the the typed, uniform one. If an invalid write is made,
    /// it can corrupt the database from the eyes of heed.
    ///
    /// # Example
    ///
    /// ```
    /// # use std::fs;
    /// # use std::path::Path;
    /// # use heed::EnvOpenOptions;
    /// use heed::{Database, PolyDatabase};
    /// use heed::types::*;
    /// use heed::byteorder::BigEndian;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dir = tempfile::tempdir()?;
    /// # let env = EnvOpenOptions::new()
    /// #     .map_size(10 * 1024 * 1024) // 10MB
    /// #     .max_dbs(3000)
    /// #     .open(dir.path())?;
    /// type BEI32 = I32<BigEndian>;
    ///
    /// let db = env.create_poly_database(Some("iter-i32"))?;
    ///
    /// let mut wtxn = env.write_txn()?;
    /// # db.clear(&mut wtxn)?;
    /// // We remap the types for ease of use.
    /// let db = db.as_uniform::<OwnedType<BEI32>, Str>();
    /// db.put(&mut wtxn, &BEI32::new(42), "i-am-forty-two")?;
    /// db.put(&mut wtxn, &BEI32::new(27), "i-am-twenty-seven")?;
    /// db.put(&mut wtxn, &BEI32::new(13), "i-am-thirteen")?;
    /// db.put(&mut wtxn, &BEI32::new(521), "i-am-five-hundred-and-twenty-one")?;
    ///
    /// wtxn.commit()?;
    /// # Ok(()) }
    /// ```
    pub fn as_uniform<KC, DC>(&self) -> Database<KC, DC> {
        Database::new(self.env_ident, self.dbi)
    }
}
