use age::Decryptor;
pub fn test_decryptor() {
    let encrypted = b"some data";
    let x = Decryptor::new(&encrypted[..]).unwrap();
    let _: () = x;
}
