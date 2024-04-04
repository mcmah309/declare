// use error_set::error_set;

//     error_set!(
//         SetLevelError {
//             IoError(std::io::Error),
//             Y
//         }
//     );

#[cfg(test)]
pub mod regular {
    use error_set::error_set;

    error_set!(
        SetLevelError {
            MagazineParsingError {
                MissingNameArg,
                MissingPublishTimeArg
            },
            BookParsingError {
                MissingNameArg,
                MissingPublishTimeArg,
                MissingDescriptionArg,
            },
        }
    );

    #[test]
    fn into_works_correctly() {
        let magazine_error = MagazineParsingError::MissingNameArg;
        let crate_error: SetLevelError = magazine_error.into();
        println!("{:?}", crate_error);
    
        let book_error = BookParsingError::MissingDescriptionArg;
        let crate_error_from_book: SetLevelError = book_error.into();
        println!("{:?}", crate_error_from_book);
    
        let x: Result<(), MagazineParsingError> = Err(MagazineParsingError::MissingNameArg);
        let _y: Result<(), BookParsingError> = x.map_err(Into::into);
    }
}

#[cfg(test)]
pub mod  empty_set {
    use error_set::error_set;

    error_set!(
        SetLevelError {
            MagazineParsingError {
                MissingNameArg,
                MissingPublishTimeArg
            },
            EmptySet1,
            BookParsingError {
                MissingNameArg,
                MissingPublishTimeArg,
                MissingDescriptionArg,
            },
            EmptySet2,
        }
    );

    #[test]
    fn test() {
        let _empty1 = SetLevelError::EmptySet1;
        let _empty2 = SetLevelError::EmptySet2;
        let book_error = BookParsingError::MissingDescriptionArg;
        let _crate_error_from_book: SetLevelError = book_error.into();
    }
}

#[cfg(test)]
pub mod  only_empty_set {
    use error_set::error_set;

    error_set!(
        SetLevelError {
            EmptySet1,
            EmptySet2,
        }
    );

    #[test]
    fn test() {
        let _empty1 = SetLevelError::EmptySet1;
        let _empty2 = SetLevelError::EmptySet2;
    }
}