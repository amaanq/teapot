use crate::{
   api::schema::{
      UserData,
      UserLegacy,
      UserResultData,
   },
   error::{
      Error,
      Result,
   },
   types::{
      User,
      VerifiedType,
   },
   utils::formatters::parse_twitter_time,
};

/// Parse a user from typed GraphQL response.
pub fn parse_user(data: &UserResultData) -> Result<User> {
   let user_data = data
      .user
      .as_ref()
      .or(data.user_result.as_ref())
      .or(data.user_results.as_ref())
      .and_then(|nested| nested.result.as_deref())
      .ok_or_else(|| Error::UserNotFound("User data not found in response".into()))?;

   parse_user_object(user_data)
}

/// Parse a user object (from `NestedResult` or direct).
pub fn parse_user_object(raw: &UserData) -> Result<User> {
   User::try_from(raw)
}

impl TryFrom<&UserData> for User {
   type Error = Error;

   fn try_from(raw: &UserData) -> Result<Self> {
      // Handle UserUnavailable
      if raw.__typename.as_deref() == Some("UserUnavailable") {
         let reason = raw
            .reason
            .clone()
            .unwrap_or_else(|| "unavailable".to_owned());
         return Err(Error::UserNotFound(reason));
      }

      let default_legacy = UserLegacy::default();
      let legacy = raw.legacy.as_ref().unwrap_or(&default_legacy);

      let id = raw
         .rest_id
         .as_deref()
         .or(legacy.id_str.as_deref())
         .unwrap_or_default()
         .to_owned();

      if id.is_empty() {
         return Err(Error::UserNotFound("No user ID found".into()));
      }

      let username = legacy.screen_name.clone().unwrap_or_default();
      let fullname = legacy.name.clone().unwrap_or_default();
      let bio = legacy.description.clone().unwrap_or_default();
      let location = legacy.location.clone().unwrap_or_default();

      let website = legacy
         .entities
         .first()
         .and_then(|url_item| url_item.expanded_url.as_deref())
         .unwrap_or_default()
         .to_owned();

      let user_pic = legacy
         .profile_image_url_https
         .as_deref()
         .unwrap_or_default()
         .replace("_normal", "_400x400");

      let banner = {
         let url = legacy.profile_banner_url.as_deref().unwrap_or_default();
         if url.is_empty() {
            let color = legacy.profile_link_color.as_deref().unwrap_or_default();
            if color.is_empty() {
               String::new()
            } else {
               format!("#{color}")
            }
         } else {
            format!("{url}/1500x500")
         }
      };

      let followers = legacy.followers_count;
      let following = legacy.friends_count;
      let tweets = legacy.statuses_count;
      let likes = legacy.favourites_count;
      let media = legacy.media_count;
      let protected = raw
         .privacy
         .as_ref()
         .map(|pr| pr.protected)
         .or(legacy.protected)
         .unwrap_or(false);
      let suspended = raw.unavailable_message;

      let is_blue = raw.is_blue_verified.unwrap_or(false);
      // Check verified_type first -- Business/Government accounts also have
      // is_blue_verified=true
      let verified_type = match legacy.verified_type.as_deref() {
         Some("Business") => VerifiedType::Business,
         Some("Government") => VerifiedType::Government,
         _ if is_blue => VerifiedType::Blue,
         _ => VerifiedType::None,
      };

      let join_date = legacy
         .created_at
         .as_deref()
         .or_else(|| {
            let core = raw.core.as_ref()?;
            core.created_at.as_deref()
         })
         .and_then(parse_twitter_time);

      let pinned_tweet = legacy
         .pinned_tweet_ids_str
         .as_ref()
         .and_then(|ids| ids.first())
         .and_then(|id_str| id_str.parse().ok())
         .unwrap_or(0);

      // Fallback to support newer GraphQL updates where fields moved out of legacy
      let (username, fullname, user_pic, bio, location, verified_type) =
         if username.is_empty() || user_pic.is_empty() {
            let fb_username = raw
               .core
               .as_ref()
               .and_then(|core| core.screen_name.as_deref())
               .unwrap_or_default()
               .to_owned();
            let fb_fullname = raw
               .core
               .as_ref()
               .and_then(|core| core.name.as_deref())
               .unwrap_or_default()
               .to_owned();
            let fb_user_pic = raw
               .avatar
               .as_ref()
               .and_then(|avatar| avatar.image_url.as_deref())
               .unwrap_or_default()
               .replace("_normal", "_400x400");
            let fb_location = raw
               .location
               .as_ref()
               .and_then(|loc| loc.location.as_deref())
               .unwrap_or_default()
               .to_owned();
            let fb_bio = raw
               .profile_bio
               .as_ref()
               .and_then(|bio_data| bio_data.description.as_deref())
               .unwrap_or_default()
               .to_owned();

            // Check verified_type first -- Business/Government override blue
            let fb_verified_type = match raw
               .verification
               .as_ref()
               .and_then(|ver| ver.verified_type.as_deref())
            {
               Some("Business") => VerifiedType::Business,
               Some("Government") => VerifiedType::Government,
               _ if is_blue => VerifiedType::Blue,
               _ => verified_type,
            };

            (
               if fb_username.is_empty() {
                  username
               } else {
                  fb_username
               },
               if fb_fullname.is_empty() {
                  fullname
               } else {
                  fb_fullname
               },
               if fb_user_pic.is_empty() {
                  user_pic
               } else {
                  fb_user_pic
               },
               if fb_bio.is_empty() { bio } else { fb_bio },
               if fb_location.is_empty() {
                  location
               } else {
                  fb_location
               },
               fb_verified_type,
            )
         } else {
            (username, fullname, user_pic, bio, location, verified_type)
         };

      Ok(Self {
         id,
         username,
         fullname,
         location,
         website,
         bio,
         user_pic,
         banner,
         pinned_tweet,
         following,
         followers,
         tweets,
         likes,
         media,
         verified_type,
         protected,
         suspended,
         join_date,
      })
   }
}
