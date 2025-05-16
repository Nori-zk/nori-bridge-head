pub use nori_token_bridge::*;
/// This module was auto-generated with ethers-rs Abigen.
/// More information at: <https://github.com/gakonst/ethers-rs>
#[allow(
    clippy::enum_variant_names,
    clippy::too_many_arguments,
    clippy::upper_case_acronyms,
    clippy::type_complexity,
    dead_code,
    non_camel_case_types,
)]
pub mod nori_token_bridge {
    #[allow(deprecated)]
    fn __abi() -> ::ethers::core::abi::Abi {
        ::ethers::core::abi::ethabi::Contract {
            constructor: ::core::option::Option::Some(::ethers::core::abi::ethabi::Constructor {
                inputs: ::std::vec![],
            }),
            functions: ::core::convert::From::from([
                (
                    ::std::borrow::ToOwned::to_owned("bridgeOperator"),
                    ::std::vec![
                        ::ethers::core::abi::ethabi::Function {
                            name: ::std::borrow::ToOwned::to_owned("bridgeOperator"),
                            inputs: ::std::vec![],
                            outputs: ::std::vec![
                                ::ethers::core::abi::ethabi::Param {
                                    name: ::std::string::String::new(),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Address,
                                    internal_type: ::core::option::Option::Some(
                                        ::std::borrow::ToOwned::to_owned("address"),
                                    ),
                                },
                            ],
                            constant: ::core::option::Option::None,
                            state_mutability: ::ethers::core::abi::ethabi::StateMutability::View,
                        },
                    ],
                ),
                (
                    ::std::borrow::ToOwned::to_owned("lockTokens"),
                    ::std::vec![
                        ::ethers::core::abi::ethabi::Function {
                            name: ::std::borrow::ToOwned::to_owned("lockTokens"),
                            inputs: ::std::vec![],
                            outputs: ::std::vec![],
                            constant: ::core::option::Option::None,
                            state_mutability: ::ethers::core::abi::ethabi::StateMutability::Payable,
                        },
                    ],
                ),
                (
                    ::std::borrow::ToOwned::to_owned("lockedTokens"),
                    ::std::vec![
                        ::ethers::core::abi::ethabi::Function {
                            name: ::std::borrow::ToOwned::to_owned("lockedTokens"),
                            inputs: ::std::vec![
                                ::ethers::core::abi::ethabi::Param {
                                    name: ::std::string::String::new(),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Address,
                                    internal_type: ::core::option::Option::Some(
                                        ::std::borrow::ToOwned::to_owned("address"),
                                    ),
                                },
                            ],
                            outputs: ::std::vec![
                                ::ethers::core::abi::ethabi::Param {
                                    name: ::std::string::String::new(),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Uint(
                                        256usize,
                                    ),
                                    internal_type: ::core::option::Option::Some(
                                        ::std::borrow::ToOwned::to_owned("uint256"),
                                    ),
                                },
                            ],
                            constant: ::core::option::Option::None,
                            state_mutability: ::ethers::core::abi::ethabi::StateMutability::View,
                        },
                    ],
                ),
            ]),
            events: ::core::convert::From::from([
                (
                    ::std::borrow::ToOwned::to_owned("TokensLocked"),
                    ::std::vec![
                        ::ethers::core::abi::ethabi::Event {
                            name: ::std::borrow::ToOwned::to_owned("TokensLocked"),
                            inputs: ::std::vec![
                                ::ethers::core::abi::ethabi::EventParam {
                                    name: ::std::borrow::ToOwned::to_owned("user"),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Address,
                                    indexed: true,
                                },
                                ::ethers::core::abi::ethabi::EventParam {
                                    name: ::std::borrow::ToOwned::to_owned("amount"),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Uint(
                                        256usize,
                                    ),
                                    indexed: false,
                                },
                                ::ethers::core::abi::ethabi::EventParam {
                                    name: ::std::borrow::ToOwned::to_owned("when"),
                                    kind: ::ethers::core::abi::ethabi::ParamType::Uint(
                                        256usize,
                                    ),
                                    indexed: false,
                                },
                            ],
                            anonymous: false,
                        },
                    ],
                ),
            ]),
            errors: ::std::collections::BTreeMap::new(),
            receive: false,
            fallback: false,
        }
    }
    ///The parsed JSON ABI of the contract.
    pub static NORITOKENBRIDGE_ABI: ::ethers::contract::Lazy<::ethers::core::abi::Abi> = ::ethers::contract::Lazy::new(
        __abi,
    );
    #[rustfmt::skip]
    const __BYTECODE: &[u8] = b"`\x80`@R4\x80\x15`\x0FW`\0\x80\xFD[P3`\0\x80a\x01\0\n\x81T\x81s\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x02\x19\x16\x90\x83s\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x16\x02\x17\x90UPa\x03\xFF\x80a\0_`\09`\0\xF3\xFE`\x80`@R`\x046\x10a\x004W`\x005`\xE0\x1C\x80c\nV)=\x14a\09W\x80cY\x06e\xC2\x14a\0CW\x80c^\xB7A:\x14a\0nW[`\0\x80\xFD[a\0Aa\0\xABV[\0[4\x80\x15a\0OW`\0\x80\xFD[Pa\0Xa\x01\x96V[`@Qa\0e\x91\x90a\x02\x13V[`@Q\x80\x91\x03\x90\xF3[4\x80\x15a\0zW`\0\x80\xFD[Pa\0\x95`\x04\x806\x03\x81\x01\x90a\0\x90\x91\x90a\x02_V[a\x01\xBAV[`@Qa\0\xA2\x91\x90a\x02\xA5V[`@Q\x80\x91\x03\x90\xF3[`\x004\x11a\0\xEEW`@Q\x7F\x08\xC3y\xA0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x81R`\x04\x01a\0\xE5\x90a\x03\x1DV[`@Q\x80\x91\x03\x90\xFD[4`\x01`\x003s\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x16s\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x16\x81R` \x01\x90\x81R` \x01`\0 `\0\x82\x82Ta\x01=\x91\x90a\x03lV[\x92PP\x81\x90UP3s\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x16\x7F\xD7A\xE78\xA2?\xD1\x8A\x03\xA2e\"2\r\x9F\xC6\xCA\xC1\xFE\xD4\x83\xE2\x15\xEA\x91P\xFB\xC2\xFCC8]4B`@Qa\x01\x8C\x92\x91\x90a\x03\xA0V[`@Q\x80\x91\x03\x90\xA2V[`\0\x80T\x90a\x01\0\n\x90\x04s\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x16\x81V[`\x01` R\x80`\0R`@`\0 `\0\x91P\x90PT\x81V[`\0s\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x82\x16\x90P\x91\x90PV[`\0a\x01\xFD\x82a\x01\xD2V[\x90P\x91\x90PV[a\x02\r\x81a\x01\xF2V[\x82RPPV[`\0` \x82\x01\x90Pa\x02(`\0\x83\x01\x84a\x02\x04V[\x92\x91PPV[`\0\x80\xFD[a\x02<\x81a\x01\xF2V[\x81\x14a\x02GW`\0\x80\xFD[PV[`\0\x815\x90Pa\x02Y\x81a\x023V[\x92\x91PPV[`\0` \x82\x84\x03\x12\x15a\x02uWa\x02ta\x02.V[[`\0a\x02\x83\x84\x82\x85\x01a\x02JV[\x91PP\x92\x91PPV[`\0\x81\x90P\x91\x90PV[a\x02\x9F\x81a\x02\x8CV[\x82RPPV[`\0` \x82\x01\x90Pa\x02\xBA`\0\x83\x01\x84a\x02\x96V[\x92\x91PPV[`\0\x82\x82R` \x82\x01\x90P\x92\x91PPV[\x7FYou must send some Ether to lock`\0\x82\x01RPV[`\0a\x03\x07` \x83a\x02\xC0V[\x91Pa\x03\x12\x82a\x02\xD1V[` \x82\x01\x90P\x91\x90PV[`\0` \x82\x01\x90P\x81\x81\x03`\0\x83\x01Ra\x036\x81a\x02\xFAV[\x90P\x91\x90PV[\x7FNH{q\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0`\0R`\x11`\x04R`$`\0\xFD[`\0a\x03w\x82a\x02\x8CV[\x91Pa\x03\x82\x83a\x02\x8CV[\x92P\x82\x82\x01\x90P\x80\x82\x11\x15a\x03\x9AWa\x03\x99a\x03=V[[\x92\x91PPV[`\0`@\x82\x01\x90Pa\x03\xB5`\0\x83\x01\x85a\x02\x96V[a\x03\xC2` \x83\x01\x84a\x02\x96V[\x93\x92PPPV\xFE\xA2dipfsX\"\x12 #\x88r\x04s\x8B`\x17\xD3\x1DXg\xF6\xD4\x15^o\xF6\xA1\xACld\x90\xF5\x95\x88\xAB\x17\xC3\xF2\xD2\xFDdsolcC\0\x08\x1C\x003";
    /// The bytecode of the contract.
    pub static NORITOKENBRIDGE_BYTECODE: ::ethers::core::types::Bytes = ::ethers::core::types::Bytes::from_static(
        __BYTECODE,
    );
    #[rustfmt::skip]
    const __DEPLOYED_BYTECODE: &[u8] = b"`\x80`@R`\x046\x10a\x004W`\x005`\xE0\x1C\x80c\nV)=\x14a\09W\x80cY\x06e\xC2\x14a\0CW\x80c^\xB7A:\x14a\0nW[`\0\x80\xFD[a\0Aa\0\xABV[\0[4\x80\x15a\0OW`\0\x80\xFD[Pa\0Xa\x01\x96V[`@Qa\0e\x91\x90a\x02\x13V[`@Q\x80\x91\x03\x90\xF3[4\x80\x15a\0zW`\0\x80\xFD[Pa\0\x95`\x04\x806\x03\x81\x01\x90a\0\x90\x91\x90a\x02_V[a\x01\xBAV[`@Qa\0\xA2\x91\x90a\x02\xA5V[`@Q\x80\x91\x03\x90\xF3[`\x004\x11a\0\xEEW`@Q\x7F\x08\xC3y\xA0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x81R`\x04\x01a\0\xE5\x90a\x03\x1DV[`@Q\x80\x91\x03\x90\xFD[4`\x01`\x003s\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x16s\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x16\x81R` \x01\x90\x81R` \x01`\0 `\0\x82\x82Ta\x01=\x91\x90a\x03lV[\x92PP\x81\x90UP3s\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x16\x7F\xD7A\xE78\xA2?\xD1\x8A\x03\xA2e\"2\r\x9F\xC6\xCA\xC1\xFE\xD4\x83\xE2\x15\xEA\x91P\xFB\xC2\xFCC8]4B`@Qa\x01\x8C\x92\x91\x90a\x03\xA0V[`@Q\x80\x91\x03\x90\xA2V[`\0\x80T\x90a\x01\0\n\x90\x04s\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x16\x81V[`\x01` R\x80`\0R`@`\0 `\0\x91P\x90PT\x81V[`\0s\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x82\x16\x90P\x91\x90PV[`\0a\x01\xFD\x82a\x01\xD2V[\x90P\x91\x90PV[a\x02\r\x81a\x01\xF2V[\x82RPPV[`\0` \x82\x01\x90Pa\x02(`\0\x83\x01\x84a\x02\x04V[\x92\x91PPV[`\0\x80\xFD[a\x02<\x81a\x01\xF2V[\x81\x14a\x02GW`\0\x80\xFD[PV[`\0\x815\x90Pa\x02Y\x81a\x023V[\x92\x91PPV[`\0` \x82\x84\x03\x12\x15a\x02uWa\x02ta\x02.V[[`\0a\x02\x83\x84\x82\x85\x01a\x02JV[\x91PP\x92\x91PPV[`\0\x81\x90P\x91\x90PV[a\x02\x9F\x81a\x02\x8CV[\x82RPPV[`\0` \x82\x01\x90Pa\x02\xBA`\0\x83\x01\x84a\x02\x96V[\x92\x91PPV[`\0\x82\x82R` \x82\x01\x90P\x92\x91PPV[\x7FYou must send some Ether to lock`\0\x82\x01RPV[`\0a\x03\x07` \x83a\x02\xC0V[\x91Pa\x03\x12\x82a\x02\xD1V[` \x82\x01\x90P\x91\x90PV[`\0` \x82\x01\x90P\x81\x81\x03`\0\x83\x01Ra\x036\x81a\x02\xFAV[\x90P\x91\x90PV[\x7FNH{q\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0`\0R`\x11`\x04R`$`\0\xFD[`\0a\x03w\x82a\x02\x8CV[\x91Pa\x03\x82\x83a\x02\x8CV[\x92P\x82\x82\x01\x90P\x80\x82\x11\x15a\x03\x9AWa\x03\x99a\x03=V[[\x92\x91PPV[`\0`@\x82\x01\x90Pa\x03\xB5`\0\x83\x01\x85a\x02\x96V[a\x03\xC2` \x83\x01\x84a\x02\x96V[\x93\x92PPPV\xFE\xA2dipfsX\"\x12 #\x88r\x04s\x8B`\x17\xD3\x1DXg\xF6\xD4\x15^o\xF6\xA1\xACld\x90\xF5\x95\x88\xAB\x17\xC3\xF2\xD2\xFDdsolcC\0\x08\x1C\x003";
    /// The deployed bytecode of the contract.
    pub static NORITOKENBRIDGE_DEPLOYED_BYTECODE: ::ethers::core::types::Bytes = ::ethers::core::types::Bytes::from_static(
        __DEPLOYED_BYTECODE,
    );
    pub struct NoriTokenBridge<M>(::ethers::contract::Contract<M>);
    impl<M> ::core::clone::Clone for NoriTokenBridge<M> {
        fn clone(&self) -> Self {
            Self(::core::clone::Clone::clone(&self.0))
        }
    }
    impl<M> ::core::ops::Deref for NoriTokenBridge<M> {
        type Target = ::ethers::contract::Contract<M>;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }
    impl<M> ::core::ops::DerefMut for NoriTokenBridge<M> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }
    impl<M> ::core::fmt::Debug for NoriTokenBridge<M> {
        fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
            f.debug_tuple(::core::stringify!(NoriTokenBridge))
                .field(&self.address())
                .finish()
        }
    }
    impl<M: ::ethers::providers::Middleware> NoriTokenBridge<M> {
        /// Creates a new contract instance with the specified `ethers` client at
        /// `address`. The contract derefs to a `ethers::Contract` object.
        pub fn new<T: Into<::ethers::core::types::Address>>(
            address: T,
            client: ::std::sync::Arc<M>,
        ) -> Self {
            Self(
                ::ethers::contract::Contract::new(
                    address.into(),
                    NORITOKENBRIDGE_ABI.clone(),
                    client,
                ),
            )
        }
        /// Constructs the general purpose `Deployer` instance based on the provided constructor arguments and sends it.
        /// Returns a new instance of a deployer that returns an instance of this contract after sending the transaction
        ///
        /// Notes:
        /// - If there are no constructor arguments, you should pass `()` as the argument.
        /// - The default poll duration is 7 seconds.
        /// - The default number of confirmations is 1 block.
        ///
        ///
        /// # Example
        ///
        /// Generate contract bindings with `abigen!` and deploy a new contract instance.
        ///
        /// *Note*: this requires a `bytecode` and `abi` object in the `greeter.json` artifact.
        ///
        /// ```ignore
        /// # async fn deploy<M: ethers::providers::Middleware>(client: ::std::sync::Arc<M>) {
        ///     abigen!(Greeter, "../greeter.json");
        ///
        ///    let greeter_contract = Greeter::deploy(client, "Hello world!".to_string()).unwrap().send().await.unwrap();
        ///    let msg = greeter_contract.greet().call().await.unwrap();
        /// # }
        /// ```
        pub fn deploy<T: ::ethers::core::abi::Tokenize>(
            client: ::std::sync::Arc<M>,
            constructor_args: T,
        ) -> ::core::result::Result<
            ::ethers::contract::builders::ContractDeployer<M, Self>,
            ::ethers::contract::ContractError<M>,
        > {
            let factory = ::ethers::contract::ContractFactory::new(
                NORITOKENBRIDGE_ABI.clone(),
                NORITOKENBRIDGE_BYTECODE.clone().into(),
                client,
            );
            let deployer = factory.deploy(constructor_args)?;
            let deployer = ::ethers::contract::ContractDeployer::new(deployer);
            Ok(deployer)
        }
        ///Calls the contract's `bridgeOperator` (0x590665c2) function
        pub fn bridge_operator(
            &self,
        ) -> ::ethers::contract::builders::ContractCall<
            M,
            ::ethers::core::types::Address,
        > {
            self.0
                .method_hash([89, 6, 101, 194], ())
                .expect("method not found (this should never happen)")
        }
        ///Calls the contract's `lockTokens` (0x0a56293d) function
        pub fn lock_tokens(&self) -> ::ethers::contract::builders::ContractCall<M, ()> {
            self.0
                .method_hash([10, 86, 41, 61], ())
                .expect("method not found (this should never happen)")
        }
        ///Calls the contract's `lockedTokens` (0x5eb7413a) function
        pub fn locked_tokens(
            &self,
            p0: ::ethers::core::types::Address,
        ) -> ::ethers::contract::builders::ContractCall<M, ::ethers::core::types::U256> {
            self.0
                .method_hash([94, 183, 65, 58], p0)
                .expect("method not found (this should never happen)")
        }
        ///Gets the contract's `TokensLocked` event
        pub fn tokens_locked_filter(
            &self,
        ) -> ::ethers::contract::builders::Event<
            ::std::sync::Arc<M>,
            M,
            TokensLockedFilter,
        > {
            self.0.event()
        }
        /// Returns an `Event` builder for all the events of this contract.
        pub fn events(
            &self,
        ) -> ::ethers::contract::builders::Event<
            ::std::sync::Arc<M>,
            M,
            TokensLockedFilter,
        > {
            self.0.event_with_filter(::core::default::Default::default())
        }
    }
    impl<M: ::ethers::providers::Middleware> From<::ethers::contract::Contract<M>>
    for NoriTokenBridge<M> {
        fn from(contract: ::ethers::contract::Contract<M>) -> Self {
            Self::new(contract.address(), contract.client())
        }
    }
    #[derive(
        Clone,
        ::ethers::contract::EthEvent,
        ::ethers::contract::EthDisplay,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    #[ethevent(name = "TokensLocked", abi = "TokensLocked(address,uint256,uint256)")]
    pub struct TokensLockedFilter {
        #[ethevent(indexed)]
        pub user: ::ethers::core::types::Address,
        pub amount: ::ethers::core::types::U256,
        pub when: ::ethers::core::types::U256,
    }
    ///Container type for all input parameters for the `bridgeOperator` function with signature `bridgeOperator()` and selector `0x590665c2`
    #[derive(
        Clone,
        ::ethers::contract::EthCall,
        ::ethers::contract::EthDisplay,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    #[ethcall(name = "bridgeOperator", abi = "bridgeOperator()")]
    pub struct BridgeOperatorCall;
    ///Container type for all input parameters for the `lockTokens` function with signature `lockTokens()` and selector `0x0a56293d`
    #[derive(
        Clone,
        ::ethers::contract::EthCall,
        ::ethers::contract::EthDisplay,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    #[ethcall(name = "lockTokens", abi = "lockTokens()")]
    pub struct LockTokensCall;
    ///Container type for all input parameters for the `lockedTokens` function with signature `lockedTokens(address)` and selector `0x5eb7413a`
    #[derive(
        Clone,
        ::ethers::contract::EthCall,
        ::ethers::contract::EthDisplay,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    #[ethcall(name = "lockedTokens", abi = "lockedTokens(address)")]
    pub struct LockedTokensCall(pub ::ethers::core::types::Address);
    ///Container type for all of the contract's call
    #[derive(Clone, ::ethers::contract::EthAbiType, Debug, PartialEq, Eq, Hash)]
    pub enum NoriTokenBridgeCalls {
        BridgeOperator(BridgeOperatorCall),
        LockTokens(LockTokensCall),
        LockedTokens(LockedTokensCall),
    }
    impl ::ethers::core::abi::AbiDecode for NoriTokenBridgeCalls {
        fn decode(
            data: impl AsRef<[u8]>,
        ) -> ::core::result::Result<Self, ::ethers::core::abi::AbiError> {
            let data = data.as_ref();
            if let Ok(decoded) = <BridgeOperatorCall as ::ethers::core::abi::AbiDecode>::decode(
                data,
            ) {
                return Ok(Self::BridgeOperator(decoded));
            }
            if let Ok(decoded) = <LockTokensCall as ::ethers::core::abi::AbiDecode>::decode(
                data,
            ) {
                return Ok(Self::LockTokens(decoded));
            }
            if let Ok(decoded) = <LockedTokensCall as ::ethers::core::abi::AbiDecode>::decode(
                data,
            ) {
                return Ok(Self::LockedTokens(decoded));
            }
            Err(::ethers::core::abi::Error::InvalidData.into())
        }
    }
    impl ::ethers::core::abi::AbiEncode for NoriTokenBridgeCalls {
        fn encode(self) -> Vec<u8> {
            match self {
                Self::BridgeOperator(element) => {
                    ::ethers::core::abi::AbiEncode::encode(element)
                }
                Self::LockTokens(element) => {
                    ::ethers::core::abi::AbiEncode::encode(element)
                }
                Self::LockedTokens(element) => {
                    ::ethers::core::abi::AbiEncode::encode(element)
                }
            }
        }
    }
    impl ::core::fmt::Display for NoriTokenBridgeCalls {
        fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
            match self {
                Self::BridgeOperator(element) => ::core::fmt::Display::fmt(element, f),
                Self::LockTokens(element) => ::core::fmt::Display::fmt(element, f),
                Self::LockedTokens(element) => ::core::fmt::Display::fmt(element, f),
            }
        }
    }
    impl ::core::convert::From<BridgeOperatorCall> for NoriTokenBridgeCalls {
        fn from(value: BridgeOperatorCall) -> Self {
            Self::BridgeOperator(value)
        }
    }
    impl ::core::convert::From<LockTokensCall> for NoriTokenBridgeCalls {
        fn from(value: LockTokensCall) -> Self {
            Self::LockTokens(value)
        }
    }
    impl ::core::convert::From<LockedTokensCall> for NoriTokenBridgeCalls {
        fn from(value: LockedTokensCall) -> Self {
            Self::LockedTokens(value)
        }
    }
    ///Container type for all return fields from the `bridgeOperator` function with signature `bridgeOperator()` and selector `0x590665c2`
    #[derive(
        Clone,
        ::ethers::contract::EthAbiType,
        ::ethers::contract::EthAbiCodec,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    pub struct BridgeOperatorReturn(pub ::ethers::core::types::Address);
    ///Container type for all return fields from the `lockedTokens` function with signature `lockedTokens(address)` and selector `0x5eb7413a`
    #[derive(
        Clone,
        ::ethers::contract::EthAbiType,
        ::ethers::contract::EthAbiCodec,
        Default,
        Debug,
        PartialEq,
        Eq,
        Hash
    )]
    pub struct LockedTokensReturn(pub ::ethers::core::types::U256);
}
