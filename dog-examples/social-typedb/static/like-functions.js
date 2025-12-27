// Like functionality for posts

// Get like information for a specific post
async function getPostLikeInfo(postId) {
    try {
        // Debug: Log the query parameters
        console.log('Getting like info for post ID:', postId);
        
        // Query to get like count for the specific post using post-id
        const likeCountData = await makeQuery('posts', `
            match
            $post isa post, has post-id "${postId}";
            $r isa reaction, has emoji "like";
            $r links (parent: $post, author: $liker);
            select $r;
        `);
        
        // Get current user dynamically
        const currentUser = await getCurrentUser();
        if (!currentUser) {
            console.error('No current user found for like status check');
            return { likeCount: 0, isLiked: false };
        }
        const currentUsername = currentUser.username;
        
        // Query to check if current user has liked this specific post
        const userLikeData = await makeQuery('posts', `
            match
            $post isa post, has post-id "${postId}";
            $person isa person, has username "${currentUsername}";
            $r isa reaction, has emoji "like";
            $r links (parent: $post, author: $person);
            select $r;
        `);
        
        
        
        if (likeCountData.ok?.answers) {
            console.log(`Post ${postId} - Found ${likeCountData.ok.answers.length} reactions:`);
            likeCountData.ok.answers.forEach((answer, index) => {
                console.log(`  Reaction ${index}:`, JSON.stringify(answer.data, null, 2));
            });
        }
        
        const likeCount = likeCountData.ok && likeCountData.ok.answers ? likeCountData.ok.answers.length : 0;
        const isLiked = userLikeData.ok && userLikeData.ok.answers && userLikeData.ok.answers.length > 0;
        
        console.log('Final like info:', { likeCount, isLiked });
        
        return { likeCount, isLiked };
    } catch (error) {
        console.error('Error getting post like info:', error);
        return { likeCount: 0, isLiked: false };
    }
}

// Toggle like status for a post
async function togglePostLike(postId) {
    try {
        console.log('=== TOGGLE LIKE CALLED ===');
        console.log('Post ID:', postId);
        
        // First check current like status
        const { isLiked } = await getPostLikeInfo(postId);
        console.log('Current like status:', isLiked);
        
        if (isLiked) {
            // Get current user for unlike operation
            const currentUser = await getCurrentUser();
            if (!currentUser) {
                console.error('No current user found for unlike operation');
                return isLiked;
            }
            const currentUsername = currentUser.username;
            
            // Unlike the post - delete the reaction
            const unlikeData = await makeQuery('posts', `
                match
                $post isa post, has post-id "${postId}";
                $person isa person, has username "${currentUsername}";
                $r isa reaction, has emoji "like";
                $r links (parent: $post, author: $person);
                delete
                $r;
            `, 'Write');
            
            if (unlikeData.ok) {
                showNotification('Post unliked', 'success');
                return false; // Now unliked
            } else {
                showNotification('Failed to unlike post', 'error');
                return isLiked; // Keep current state
            }
        } else {
            // Like the post - insert new reaction
            console.log('Inserting like for post:', postId);
            
            // Get current user for like operation
            const currentUser = await getCurrentUser();
            if (!currentUser) {
                console.error('No current user found for like operation');
                showNotification('User not found - cannot like post', 'error');
                return isLiked;
            }
            const validUsername = currentUser.username;
            console.log('Using username:', validUsername);
            
            const likeData = await makeQuery('posts', `
                match
                $post isa post, has post-id "${postId}";
                $person isa person, has username "${validUsername}";
                insert
                $r isa reaction (parent: $post, author: $person), 
                  has emoji "like",
                  has creation-timestamp ${new Date().toISOString().slice(0, 19)};
            `, 'Write');
            
            console.log('Insert result:', likeData);
            console.log('Insert result details:', JSON.stringify(likeData, null, 2));
            
            if (likeData.ok) {
                showNotification('Post liked', 'success');
                console.log('Like inserted successfully, checking new count...');
                
                // Verify the insert worked by querying again
                const verifyData = await makeQuery('posts', `
                    match
                    $post isa post, has post-id "${postId}";
                    $r isa reaction, has emoji "like";
                    $r links (parent: $post, author: $liker);
                    select $r;
                `);
                console.log('Verification query after insert:', verifyData);
                if (verifyData.ok?.answers) {
                    console.log(`After insert - Found ${verifyData.ok.answers.length} reactions:`);
                    verifyData.ok.answers.forEach((answer, index) => {
                        console.log(`  Verification Reaction ${index}:`, JSON.stringify(answer.data, null, 2));
                    });
                }
                
                return true; // Now liked
            } else {
                console.error('Failed to insert like:', likeData);
                showNotification('Failed to like post', 'error');
                return isLiked; // Keep current state
            }
        }
    } catch (error) {
        console.error('Error toggling post like:', error);
        showNotification('Error updating like status', 'error');
        return false;
    }
}

// Track a view for a specific post (non-blocking)
async function trackPostView(postId) {
    // Run view tracking in background without blocking feed loading
    setTimeout(async () => {
        try {
            // Get current user dynamically
            const currentUser = await getCurrentUser();
            if (!currentUser) {
                return;
            }
            
            const currentUsername = currentUser.username;
            
            // Check if this user has already viewed this post to avoid duplicate views
            const existingViewCheck = await makeQuery('posts', `
                match
                $post isa post, has post-id "${postId}";
                $person isa person, has username "${currentUsername}";
                $v isa viewing;
                $v links (viewed: $post, viewer: $person);
                select $v;
            `);
            
            if (existingViewCheck.ok?.answers && existingViewCheck.ok.answers.length > 0) {
                // User has already viewed this post, don't track again
                return;
            }
            
            // Track the view by inserting a viewing relation
            await makeQuery('posts', `
                match
                $post isa post, has post-id "${postId}";
                $person isa person, has username "${currentUsername}";
                insert
                $v isa viewing (viewed: $post, viewer: $person);
            `, 'Write');
        } catch (error) {
            // Silently fail to avoid breaking anything
        }
    }, 100); // Small delay to not block feed loading
}

// Get view count for a specific post
async function getPostViewCount(postId) {
    try {
        // Query to get view count for the specific post using viewing relation
        const viewCountData = await makeQuery('posts', `
            match
            $post isa post, has post-id "${postId}";
            $v isa viewing;
            $v links (viewed: $post, viewer: $person);
            select $v;
        `);
        
        const viewCount = viewCountData.ok && viewCountData.ok.answers ? viewCountData.ok.answers.length : 0;
        return viewCount;
    } catch (error) {
        console.error('Error getting post view count for', postId, ':', error);
        return 0;
    }
}

// Get comment count for a specific post
async function getPostCommentCount(postId) {
    try {
        // Query to get comment count for the specific post using commenting relation
        const commentCountData = await makeQuery('posts', `
            match
            $post isa post, has post-id "${postId}";
            $commenting isa commenting;
            $commenting links (parent: $post, comment: $comment, author: $author);
            $comment has comment-id $commentId;
            select $comment, $commentId;
        `);
        
        if (commentCountData.ok && commentCountData.ok.answers) {
            // Remove duplicates based on comment ID to get accurate count
            const seenCommentIds = new Set();
            const uniqueComments = commentCountData.ok.answers.filter(answer => {
                const commentId = answer.data.commentId?.value || answer.data.commentId;
                if (seenCommentIds.has(commentId)) {
                    return false;
                }
                seenCommentIds.add(commentId);
                return true;
            });
            
            return uniqueComments.length;
        }
        
        return 0;
    } catch (error) {
        console.error('Error getting post comment count for', postId, ':', error);
        return 0;
    }
}

// Get current user - now supports dynamic user switching
async function getCurrentUser() {
    try {
        // Check if user is stored in localStorage
        const storedUserId = localStorage.getItem('currentUserId');
        
        if (storedUserId) {
            // Get the stored user from database
            const userData = await makeQuery('posts', `
                match $person isa person, has username "${storedUserId}", has name $name;
                select $person, $name;
            `);
            
            if (userData.ok?.answers && userData.ok.answers.length > 0) {
                return {
                    username: storedUserId,
                    name: userData.ok.answers[0].data.name?.value || userData.ok.answers[0].data.name
                };
            }
        }
        
        // Fallback to Jason Clark if no stored user or stored user not found
        const fallbackData = await makeQuery('posts', `
            match $person isa person, has name "Jason Clark", has username $username;
            select $person, $username;
        `);
        
        if (fallbackData.ok?.answers && fallbackData.ok.answers.length > 0) {
            const username = fallbackData.ok.answers[0].data.username?.value || fallbackData.ok.answers[0].data.username;
            // Store as default user
            localStorage.setItem('currentUserId', username);
            return {
                username: username,
                name: "Jason Clark"
            };
        }
        
        // Final fallback to first available user
        const firstUserData = await makeQuery('posts', `
            match $person isa person, has username $username, has name $name;
            select $person, $username, $name;
            limit 1;
        `);
        
        if (firstUserData.ok?.answers && firstUserData.ok.answers.length > 0) {
            const username = firstUserData.ok.answers[0].data.username?.value || firstUserData.ok.answers[0].data.username;
            const name = firstUserData.ok.answers[0].data.name?.value || firstUserData.ok.answers[0].data.name;
            localStorage.setItem('currentUserId', username);
            return { username, name };
        }
        
        return null;
    } catch (error) {
        console.error('Error getting current user:', error);
        return null;
    }
}

// Switch to a different user
async function switchUser(username) {
    try {
        // Verify user exists
        const userData = await makeQuery('posts', `
            match $person isa person, has username "${username}", has name $name;
            select $person, $name;
        `);
        
        if (userData.ok?.answers && userData.ok.answers.length > 0) {
            // Store new user in localStorage
            localStorage.setItem('currentUserId', username);
            
            // Update UI to reflect new user
            await updateUserProfile();
            
            // Refresh feed to show correct ownership buttons
            await loadFeed();
            
            showNotification(`Switched to ${userData.ok.answers[0].data.name?.value || userData.ok.answers[0].data.name}`, 'success');
            return true;
        } else {
            showNotification('User not found', 'error');
            return false;
        }
    } catch (error) {
        console.error('Error switching user:', error);
        showNotification('Error switching user', 'error');
        return false;
    }
}

// Get all available users for switching with pagination
async function getAllUsers(limit = 50, offset = 0) {
    try {
        const usersData = await makeQuery('posts', `
            match $person isa person, has username $username, has name $name;
            select $person, $username, $name;
            offset ${offset}; limit ${limit};
        `);
        
        if (usersData.ok?.answers) {
            return usersData.ok.answers.map(answer => ({
                username: answer.data.username?.value || answer.data.username,
                name: answer.data.name?.value || answer.data.name
            }));
        }
        
        return [];
    } catch (error) {
        console.error('Error getting all users:', error);
        return [];
    }
}

// Get total user count for pagination
async function getUserCount() {
    try {
        const countData = await makeQuery('posts', `
            match $person isa person;
            select $person;
        `);
        
        return countData.ok?.answers?.length || 0;
    } catch (error) {
        console.error('Error getting user count:', error);
        return 0;
    }
}

// Get all available groups with member counts
async function getAllGroups() {
    try {
        const groupsData = await makeQuery('groups', `
            match $group isa group, has group-id $groupId, has name $name, has page-visibility $visibility;
            select $group, $groupId, $name, $visibility;
        `);
        
        if (groupsData.ok?.answers) {
            // Remove duplicates based on groupId
            const uniqueGroups = [];
            const seenGroupIds = new Set();
            
            for (const answer of groupsData.ok.answers) {
                const groupId = answer.data.groupId?.value || answer.data.groupId;
                if (!seenGroupIds.has(groupId)) {
                    seenGroupIds.add(groupId);
                    
                    // Get member count for this group
                    const memberCount = await getGroupMemberCount(groupId);
                    
                    uniqueGroups.push({
                        groupId: groupId,
                        name: answer.data.name?.value || answer.data.name,
                        visibility: answer.data.visibility?.value || answer.data.visibility,
                        memberCount: memberCount
                    });
                }
            }
            
            return uniqueGroups;
        }
        
        return [];
    } catch (error) {
        console.error('Error getting all groups:', error);
        return [];
    }
}

// Get member count for a specific group
async function getGroupMemberCount(groupId) {
    try {
        const memberData = await makeQuery('groups', `
            match 
            $group isa group, has group-id "${groupId}";
            $membership isa group-membership;
            $membership links (member: $person, group: $group);
            select $person;
        `);
        
        return memberData.ok?.answers?.length || 0;
    } catch (error) {
        console.error('Error getting group member count:', error);
        return 0;
    }
}

// Get group members with their details
async function getGroupMembers(groupId, limit = 5) {
    try {
        const memberData = await makeQuery('groups', `
            match 
            $group isa group, has group-id "${groupId}";
            $membership isa group-membership;
            $membership links (member: $person, group: $group);
            $person has name $name, has username $username;
            select $person, $name, $username;
            limit ${limit};
        `);
        
        if (memberData.ok?.answers) {
            return memberData.ok.answers.map(answer => ({
                name: answer.data.name?.value || answer.data.name,
                username: answer.data.username?.value || answer.data.username
            }));
        }
        
        return [];
    } catch (error) {
        console.error('Error getting group members:', error);
        return [];
    }
}

// Get user's group memberships
async function getUserGroups(username) {
    try {
        const membershipData = await makeQuery('groups', `
            match 
            $person isa person, has username "${username}";
            $membership isa group-membership;
            $membership links (member: $person, group: $group);
            $group has group-id $groupId, has name $name;
            $membership has rank $rank;
            select $group, $groupId, $name, $rank;
        `);
        
        if (membershipData.ok?.answers) {
            // Remove duplicates based on groupId
            const uniqueGroups = [];
            const seenGroupIds = new Set();
            
            membershipData.ok.answers.forEach(answer => {
                const groupId = answer.data.groupId?.value || answer.data.groupId;
                if (!seenGroupIds.has(groupId)) {
                    seenGroupIds.add(groupId);
                    uniqueGroups.push({
                        groupId: groupId,
                        name: answer.data.name?.value || answer.data.name,
                        rank: answer.data.rank?.value || answer.data.rank || 'member'
                    });
                }
            });
            
            return uniqueGroups;
        }
        
        return [];
    } catch (error) {
        console.error('Error getting user groups:', error);
        return [];
    }
}

// Join a group
async function joinGroup(groupId) {
    try {
        const currentUser = await getCurrentUser();
        if (!currentUser) {
            console.error('No current user found for joining group');
            return false;
        }
        
        const joinData = await makeQuery('groups', `
            match
            $person isa person, has username "${currentUser.username}";
            $group isa group, has group-id "${groupId}";
            insert
            $membership isa group-membership;
            $membership links (member: $person, group: $group);
            $membership has rank "member";
            $membership has start-timestamp ${new Date().toISOString().slice(0, 19)};
        `, 'Write');
        
        if (joinData.ok) {
            showNotification('Successfully joined group', 'success');
            return true;
        } else {
            showNotification('Failed to join group', 'error');
            return false;
        }
    } catch (error) {
        console.error('Error joining group:', error);
        showNotification('Error joining group', 'error');
        return false;
    }
}

// Leave a group
async function leaveGroup(groupId) {
    try {
        const currentUser = await getCurrentUser();
        if (!currentUser) {
            console.error('No current user found for leaving group');
            return false;
        }
        
        const leaveData = await makeQuery('groups', `
            match
            $person isa person, has username "${currentUser.username}";
            $group isa group, has group-id "${groupId}";
            $membership isa group-membership;
            $membership links (member: $person, group: $group);
            delete
            $membership;
        `, 'Write');
        
        if (leaveData.ok) {
            showNotification('Successfully left group', 'success');
            return true;
        } else {
            showNotification('Failed to leave group', 'error');
            return false;
        }
    } catch (error) {
        console.error('Error leaving group:', error);
        showNotification('Error leaving group', 'error');
        return false;
    }
}

// Create a new group
async function createGroup(name, visibility = 'public') {
    try {
        const currentUser = await getCurrentUser();
        if (!currentUser) {
            console.error('No current user found for creating group');
            return false;
        }
        
        const groupId = `group_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
        
        const createData = await makeQuery('groups', `
            match
            $person isa person, has username "${currentUser.username}";
            insert
            $group isa group;
            $group has group-id "${groupId}";
            $group has name "${name}";
            $group has page-visibility "${visibility}";
            $group has post-visibility "default";
            $membership isa group-membership;
            $membership links (member: $person, group: $group);
            $membership has rank "owner";
            $membership has start-timestamp ${new Date().toISOString().slice(0, 19)};
        `, 'Write');
        
        if (createData.ok) {
            showNotification('Group created successfully', 'success');
            return { success: true, groupId };
        } else {
            showNotification('Failed to create group', 'error');
            return { success: false };
        }
    } catch (error) {
        console.error('Error creating group:', error);
        showNotification('Error creating group', 'error');
        return { success: false };
    }
}

// Add a comment to a specific post
async function addComment(postId, commentText) {
    try {
        // Get current user dynamically
        const currentUser = await getCurrentUser();
        
        if (!currentUser) {
            console.error('No current user found for commenting');
            return false;
        }
        
        const currentUsername = currentUser.username;
        
        // Generate unique comment ID
        const commentId = `comment_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
        
        // Insert comment entity and commenting relation
        const commentData = await makeQuery('posts', `
            match
            $post isa post, has post-id "${postId}";
            $person isa person, has username "${currentUsername}";
            insert
            $comment isa comment, 
                has comment-id "${commentId}",
                has comment-text "${commentText}",
                has creation-timestamp ${new Date().toISOString().slice(0, 19)};
            $commenting isa commenting (parent: $post, comment: $comment, author: $person);
        `, 'Write');
        
        if (commentData.ok) {
            console.log('Comment added successfully:', commentId);
            showNotification('Comment added', 'success');
            return true;
        } else {
            console.error('Failed to add comment:', commentData);
            showNotification('Failed to add comment', 'error');
            return false;
        }
    } catch (error) {
        console.error('Error adding comment:', error);
        showNotification('Error adding comment', 'error');
        return false;
    }
}

// Get comments for a specific post
async function getPostComments(postId) {
    try {
        // Query to get all comments for the post with author information
        const commentsData = await makeQuery('posts', `
            match
            $post isa post, has post-id "${postId}";
            $commenting isa commenting;
            $commenting links (parent: $post, comment: $comment, author: $author);
            $comment has comment-text $text, has creation-timestamp $time, has comment-id $commentId;
            $author has name $authorName;
            select $comment, $text, $time, $authorName, $commentId;
        `);
        
        if (commentsData.ok && commentsData.ok.answers) {
            // Remove duplicates based on comment ID
            const seenCommentIds = new Set();
            const uniqueComments = commentsData.ok.answers.filter(answer => {
                const commentId = answer.data.commentId?.value || answer.data.commentId;
                if (seenCommentIds.has(commentId)) {
                    return false;
                }
                seenCommentIds.add(commentId);
                return true;
            });
            
            const comments = uniqueComments.map(answer => ({
                id: answer.data.commentId?.value || answer.data.commentId || '',
                text: answer.data.text?.value || answer.data.text || '',
                authorName: answer.data.authorName?.value || answer.data.authorName || 'Unknown',
                timestamp: answer.data.time?.value || answer.data.time || new Date().toISOString()
            }));
            
            // Sort comments by timestamp (newest first)
            comments.sort((a, b) => new Date(b.timestamp) - new Date(a.timestamp));
            
            return comments;
        }
        
        return [];
    } catch (error) {
        console.error('Error getting post comments:', error);
        return [];
    }
}

// Create a new post
async function createPost(postText, postType = 'text-post') {
    try {
        // Get current user dynamically
        const currentUser = await getCurrentUser();
        
        if (!currentUser) {
            console.error('No current user found for post creation');
            return false;
        }
        
        const currentUsername = currentUser.username;
        
        // Generate unique post ID
        const postId = `post_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
        
        // Insert post entity and posting relation
        const postData = await makeQuery('posts', `
            match
            $person isa person, has username "${currentUsername}";
            insert
            $post isa ${postType}, 
                has post-id "${postId}",
                has post-text "${postText}",
                has creation-timestamp ${new Date().toISOString().slice(0, 19)},
                has language "en";
            $posting isa posting (author: $person, page: $person, post: $post);
        `, 'Write');
        
        if (postData.ok) {
            console.log('Post created successfully:', postId);
            showNotification('Post created successfully', 'success');
            return { success: true, postId };
        } else {
            console.error('Failed to create post:', postData);
            showNotification('Failed to create post', 'error');
            return { success: false };
        }
    } catch (error) {
        console.error('Error creating post:', error);
        showNotification('Error creating post', 'error');
        return { success: false };
    }
}

// Delete a post
async function deletePost(postId) {
    try {
        // Get current user to verify ownership
        const currentUser = await getCurrentUser();
        
        if (!currentUser) {
            console.error('No current user found for post deletion');
            return false;
        }
        
        const currentUsername = currentUser.username;
        
        // Delete post and all related data (posting relations, reactions, comments, views)
        const deleteData = await makeQuery('posts', `
            match
            $post isa post, has post-id "${postId}";
            $author has username "${currentUsername}";
            $posting isa posting;
            $posting links (author: $author, page: $page, post: $post);
            delete
            $posting;
            $post;
        `, 'Write');
        
        if (deleteData.ok) {
            console.log('Post deleted successfully:', postId);
            showNotification('Post deleted successfully', 'success');
            return true;
        } else {
            console.error('Failed to delete post:', deleteData);
            showNotification('Failed to delete post', 'error');
            return false;
        }
    } catch (error) {
        console.error('Error deleting post:', error);
        showNotification('Error deleting post', 'error');
        return false;
    }
}

// Edit a post
async function editPost(postId, newText) {
    try {
        // Get current user to verify ownership
        const currentUser = await getCurrentUser();
        
        if (!currentUser) {
            console.error('No current user found for post editing');
            return false;
        }
        
        const currentUsername = currentUser.username;
        
        // Update post text
        const editData = await makeQuery('posts', `
            match
            $post isa post, has post-id "${postId}", has post-text $oldText;
            $posting isa posting;
            $posting links (author: $author, page: $page, post: $post);
            $author has username "${currentUsername}";
            delete
            $oldText;
            insert
            $post has post-text "${newText}";
        `, 'Write');
        
        if (editData.ok) {
            console.log('Post edited successfully:', postId);
            showNotification('Post updated successfully', 'success');
            return true;
        } else {
            console.error('Failed to edit post:', editData);
            showNotification('Failed to update post', 'error');
            return false;
        }
    } catch (error) {
        console.error('Error editing post:', error);
        showNotification('Error updating post', 'error');
        return false;
    }
}

// Edit a comment
async function editComment(commentId, newText) {
    try {
        // Get current user to verify ownership
        const currentUser = await getCurrentUser();
        
        if (!currentUser) {
            console.error('No current user found for comment editing');
            return false;
        }
        
        const currentUsername = currentUser.username;
        
        // Update comment text
        const editData = await makeQuery('posts', `
            match
            $comment isa comment, has comment-id "${commentId}", has comment-text $oldText;
            $commenting isa commenting;
            $commenting links (parent: $post, comment: $comment, author: $author);
            $author has username "${currentUsername}";
            delete
            $oldText;
            insert
            $comment has comment-text "${newText}";
        `, 'Write');
        
        if (editData.ok) {
            console.log('Comment edited successfully:', commentId);
            showNotification('Comment updated successfully', 'success');
            return true;
        } else {
            console.error('Failed to edit comment:', editData);
            showNotification('Failed to update comment', 'error');
            return false;
        }
    } catch (error) {
        console.error('Error editing comment:', error);
        showNotification('Error updating comment', 'error');
        return false;
    }
}

// Delete a comment
async function deleteComment(commentId) {
    try {
        // Get current user to verify ownership
        const currentUser = await getCurrentUser();
        
        if (!currentUser) {
            console.error('No current user found for comment deletion');
            return false;
        }
        
        const currentUsername = currentUser.username;
        
        // Delete comment and commenting relation
        const deleteData = await makeQuery('posts', `
            match
            $comment isa comment, has comment-id "${commentId}";
            $commenting isa commenting;
            $commenting links (parent: $post, comment: $comment, author: $author);
            $author has username "${currentUsername}";
            delete
            $commenting;
            $comment;
        `, 'Write');
        
        if (deleteData.ok) {
            console.log('Comment deleted successfully:', commentId);
            showNotification('Comment deleted successfully', 'success');
            return true;
        } else {
            console.error('Failed to delete comment:', deleteData);
            showNotification('Failed to delete comment', 'error');
            return false;
        }
    } catch (error) {
        console.error('Error deleting comment:', error);
        showNotification('Error deleting comment', 'error');
        return false;
    }
}

// Update comment button UI with new count
function updateCommentButton(postId, commentCount) {
    const commentButton = document.querySelector(`.comment-btn[data-post-id="${postId}"]`);
    if (!commentButton) return;

    // Find existing comment count span or create new one
    let commentCountSpan = commentButton.querySelector('.comment-count');
    
    if (commentCount > 0) {
        if (!commentCountSpan) {
            // Create new comment count span
            commentCountSpan = document.createElement('span');
            commentCountSpan.className = 'comment-count text-sm';
            commentButton.appendChild(commentCountSpan);
        }
        commentCountSpan.textContent = commentCount;
    } else {
        // Remove comment count span if count is 0
        if (commentCountSpan) {
            commentCountSpan.remove();
        }
    }
}

// Update like button UI
function updateLikeButton(button, isLiked, likeCount) {
    const icon = button.querySelector('i');
    const countSpan = button.querySelector('.like-count');
    
    // Update icon
    if (isLiked) {
        icon.className = 'fas fa-thumbs-up text-lg';
        button.classList.remove('text-neutral-500', 'hover:text-blue-600');
        button.classList.add('text-blue-600');
    } else {
        icon.className = 'far fa-thumbs-up text-lg';
        button.classList.remove('text-blue-600');
        button.classList.add('text-neutral-500', 'hover:text-blue-600');
    }
    
    // Update count
    if (countSpan) {
        if (likeCount > 0) {
            countSpan.textContent = likeCount;
        } else {
            countSpan.remove();
        }
    } else if (likeCount > 0) {
        // Add count span if it doesn't exist
        const countElement = document.createElement('span');
        countElement.className = 'like-count text-sm';
        countElement.textContent = likeCount;
        button.appendChild(countElement);
    }
}
