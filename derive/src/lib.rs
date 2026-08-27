//! `#[derive(DeepClone)]` for the [`deepclone`](https://docs.rs/deepclone) crate.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
	Data, DeriveInput, Expr, Fields, Index, Path, WhereClause, parse_macro_input, parse_quote,
	punctuated::Punctuated, spanned::Spanned,
};

/// Derive `DeepClone`, cloning every field through the same `Cloner`.
///
/// Structs (named, tuple, and unit), enums, generics, and where-clauses are supported; unions
/// are not. Every type parameter gains a `DeepClone` bound, as `derive(Clone)` adds a `Clone`
/// bound.
///
/// Nothing here inspects field types: `Rc` and `Arc` route through the memo because their own
/// `DeepClone` impls do, so `Vec<Rc<T>>`, `Option<Rc<T>>`, and `HashMap<K, Rc<T>>` work too —
/// all three of which a derive matching on the literal token `Rc` would miss.
///
/// No `'static` bound is added, since only types actually holding an `Rc` or `Arc` need one. A
/// generic type with an `Rc<..T..>` field therefore needs `T: 'static` on its declaration.
///
/// # Field attributes
///
/// - `#[deepclone(clone)]` — use `Clone::clone`. Correct for immutable or unshared data,
///   never for an `Rc` you want independent.
/// - `#[deepclone(with = path)]` — call `path(&field, cloner)`.
/// - `#[deepclone(default)]` — ignore the source value and use `Default::default()`.
///
/// # Container attributes
///
/// - `#[deepclone(bound = "T: MyBound")]` — replace the generated bounds, for when a
///   `DeepClone` bound on every parameter is too strong.
#[proc_macro_derive(DeepClone, attributes(deepclone))]
pub fn derive_deep_clone(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as DeriveInput);
	expand(&input)
		.unwrap_or_else(syn::Error::into_compile_error)
		.into()
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
	let body = match &input.data {
		Data::Struct(data) => {
			clone_fields(&quote!(Self), &data.fields, &|member| quote!(&self.#member))?
		}
		Data::Enum(data) => {
			let arms = data
				.variants
				.iter()
				.map(|variant| {
					let name = &variant.ident;
					let bindings = bind_fields(&variant.fields);
					let fields = clone_fields(&quote!(Self::#name), &variant.fields, &|member| {
						let binding = binding_ident(&member);
						quote!(#binding)
					})?;
					Ok(quote!(Self::#name #bindings => #fields))
				})
				.collect::<syn::Result<Vec<_>>>()?;
			// An enum with no variants is uninhabited, so `match` on it needs no arms.
			quote!(match self { #(#arms,)* })
		}
		Data::Union(data) => {
			return Err(syn::Error::new(
				data.union_token.span(),
				"`DeepClone` cannot be derived for unions, because which field is live is not \
                 known statically",
			));
		}
	};

	let name = &input.ident;
	let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
	let where_clause = match container_bound(input)? {
		Some(bound) => bound,
		None => {
			let mut clause = where_clause.cloned().unwrap_or_else(|| parse_quote!(where));
			for param in input.generics.type_params() {
				let param = &param.ident;
				clause
					.predicates
					.push(parse_quote!(#param: ::deepclone::DeepClone));
			}
			clause
		}
	};

	Ok(quote! {
		#[automatically_derived]
		impl #impl_generics ::deepclone::DeepClone for #name #ty_generics #where_clause {
			fn deep_clone_in(&self, cloner: &mut ::deepclone::Cloner) -> Self {
				#body
			}
		}
	})
}

/// The name a variant's field is bound to, so named and tuple variants share one builder.
fn binding_ident(member: &TokenStream2) -> proc_macro2::Ident {
	format_ident!("field_{}", member.to_string().replace(['.', ' '], "_"))
}

/// The pattern that binds a variant's fields, `{ a: field_a, .. }` or `(field_0, ..)`.
fn bind_fields(fields: &Fields) -> TokenStream2 {
	match fields {
		Fields::Named(named) => {
			let bindings = named.named.iter().map(|field| {
				let name = field.ident.as_ref().expect("named field has an identifier");
				let binding = binding_ident(&quote!(#name));
				quote!(#name: #binding)
			});
			quote!({ #(#bindings,)* })
		}
		Fields::Unnamed(unnamed) => {
			let bindings = (0..unnamed.unnamed.len()).map(|index| {
				let index = Index::from(index);
				binding_ident(&quote!(#index))
			});
			quote!((#(#bindings,)*))
		}
		Fields::Unit => quote!(),
	}
}

/// Build `ctor { .. }` / `ctor(..)` / `ctor` from a per-field accessor, where `ctor` is
/// `Self` for a struct and `Self::Variant` for an enum variant.
fn clone_fields(
	ctor: &TokenStream2,
	fields: &Fields,
	access: &dyn Fn(TokenStream2) -> TokenStream2,
) -> syn::Result<TokenStream2> {
	Ok(match fields {
		Fields::Named(named) => {
			let values = named
				.named
				.iter()
				.map(|field| {
					let name = field.ident.as_ref().expect("named field has an identifier");
					let value = field_expr(field, access(quote!(#name)))?;
					Ok(quote!(#name: #value))
				})
				.collect::<syn::Result<Vec<_>>>()?;
			quote!(#ctor { #(#values,)* })
		}
		Fields::Unnamed(unnamed) => {
			let values = unnamed
				.unnamed
				.iter()
				.enumerate()
				.map(|(index, field)| {
					let index = Index::from(index);
					field_expr(field, access(quote!(#index)))
				})
				.collect::<syn::Result<Vec<_>>>()?;
			quote!(#ctor(#(#values,)*))
		}
		Fields::Unit => quote!(#ctor),
	})
}

/// How one field is cloned, after its `#[deepclone(..)]` attribute is applied.
enum Strategy {
	/// Recurse, threading the cloner. Anything reached through here keeps its sharing.
	Deep,
	/// Shallow `Clone::clone`, opted into explicitly at the field.
	Clone,
	/// A user-supplied `fn(&Field, &mut Cloner) -> Field`.
	With(Path),
	/// Ignore the source value entirely.
	Default,
}

fn field_expr(field: &syn::Field, access: TokenStream2) -> syn::Result<TokenStream2> {
	Ok(match field_strategy(field)? {
		Strategy::Deep => quote!(::deepclone::DeepClone::deep_clone_in(#access, cloner)),
		Strategy::Clone => quote!(::core::clone::Clone::clone(#access)),
		Strategy::With(path) => quote!(#path(#access, cloner)),
		Strategy::Default => quote!(::core::default::Default::default()),
	})
}

fn field_strategy(field: &syn::Field) -> syn::Result<Strategy> {
	let mut strategy = None;
	for attr in field
		.attrs
		.iter()
		.filter(|attr| attr.path().is_ident("deepclone"))
	{
		attr.parse_nested_meta(|meta| {
			let found = if meta.path.is_ident("clone") {
				Strategy::Clone
			} else if meta.path.is_ident("default") {
				Strategy::Default
			} else if meta.path.is_ident("with") {
				Strategy::With(meta.value()?.parse()?)
			} else {
				return Err(meta.error(
					"unknown `deepclone` field attribute, expected `clone`, `default`, or `with`",
				));
			};
			if strategy.is_some() {
				return Err(meta.error("conflicting `deepclone` field attributes"));
			}
			strategy = Some(found);
			Ok(())
		})?;
	}
	Ok(strategy.unwrap_or(Strategy::Deep))
}

/// Read a container-level `#[deepclone(bound = "..")]`, which replaces the generated bounds.
fn container_bound(input: &DeriveInput) -> syn::Result<Option<WhereClause>> {
	let mut bound = None;
	for attr in input
		.attrs
		.iter()
		.filter(|attr| attr.path().is_ident("deepclone"))
	{
		attr.parse_nested_meta(|meta| {
			if !meta.path.is_ident("bound") {
				return Err(meta
					.error("unknown `deepclone` container attribute, expected `bound = \"..\"`"));
			}
			let Expr::Lit(syn::ExprLit {
				lit: syn::Lit::Str(text),
				..
			}) = meta.value()?.parse::<Expr>()?
			else {
				return Err(meta.error("`bound` expects a string, as in `bound = \"T: Copy\"`"));
			};
			let predicates = text.parse_with(Punctuated::parse_terminated)?;
			bound = Some(WhereClause {
				where_token: Default::default(),
				predicates,
			});
			Ok(())
		})?;
	}
	Ok(bound)
}
