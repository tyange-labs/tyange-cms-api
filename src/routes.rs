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
pub mod login;
pub mod me;
pub mod signup;
pub mod update_my_profile;
pub mod update_post;
pub mod upload_image;
pub mod upload_post;

#[cfg(test)]
mod portfolio_routes_test;
#[cfg(test)]
mod post_authorization_test;
#[cfg(test)]
mod signup_test;
#[cfg(test)]
mod upload_image_test;
#[cfg(test)]
mod wedding_guest_test;
