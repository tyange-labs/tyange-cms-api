pub mod add_user;
pub mod create_wedding_guest;
pub mod delete_post;
pub mod get_all_posts;
pub mod get_count_with_tags;
pub mod get_post;
pub mod get_posts;
pub mod get_posts_with_tags;
pub mod get_tags_with_category;
pub mod get_wedding_guests;
pub mod gotolib_book_info;
pub mod gotolib_covers;
pub mod gotolib_push;
pub mod gotolib_sync;
pub mod login;
pub mod me;
pub mod passkeys;
pub mod update_my_profile;
pub mod update_post;
pub mod upload_image;
pub mod upload_post;

#[cfg(test)]
mod api_keys_test;
#[cfg(test)]
mod gotolib_book_info_test;
#[cfg(test)]
mod gotolib_covers_test;
#[cfg(test)]
mod gotolib_push_test;
#[cfg(test)]
mod gotolib_role_test;
#[cfg(test)]
mod gotolib_sync_test;
#[cfg(test)]
mod passkeys_test;
#[cfg(test)]
mod portfolio_routes_test;
#[cfg(test)]
mod post_delete_test;
#[cfg(test)]
mod post_authorization_test;
#[cfg(test)]
mod account_test;
#[cfg(test)]
mod upload_image_test;
#[cfg(test)]
mod wedding_guest_test;
