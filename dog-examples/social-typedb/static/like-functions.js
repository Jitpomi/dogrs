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
        
        // Get current user (first available user for now)
        const currentUserCheck = await makeQuery('posts', `
            match $person isa person, has username $username;
            select $person, $username;
            limit 1;
        `);
        
        let currentUsername = "jasonc"; // fallback
        if (currentUserCheck.ok?.answers && currentUserCheck.ok.answers.length > 0) {
            currentUsername = currentUserCheck.ok.answers[0].data.username?.value || currentUserCheck.ok.answers[0].data.username;
        }
        
        // Query to check if current user has liked this specific post
        const userLikeData = await makeQuery('posts', `
            match
            $post isa post, has post-id "${postId}";
            $person isa person, has username "${currentUsername}";
            $r isa reaction, has emoji "like";
            $r links (parent: $post, author: $person);
            select $r;
        `);
        
        // Debug: Log the like results
        console.log('Like count query result:', likeCountData);
        console.log('User like query result:', userLikeData);
        
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
            const currentUserCheck = await makeQuery('posts', `
                match $person isa person, has username $username;
                select $person, $username;
                limit 1;
            `);
            
            let currentUsername = "jasonc"; // fallback
            if (currentUserCheck.ok?.answers && currentUserCheck.ok.answers.length > 0) {
                currentUsername = currentUserCheck.ok.answers[0].data.username?.value || currentUserCheck.ok.answers[0].data.username;
            }
            
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
            
            // First find existing users to get a valid username
            const allUsersCheck = await makeQuery('posts', `
                match $person isa person, has username $username;
                select $person, $username;
            `);
            console.log('All users in database:', allUsersCheck);
            
            if (!allUsersCheck.ok?.answers || allUsersCheck.ok.answers.length === 0) {
                console.error('No users found in database!');
                showNotification('No users found - cannot like post', 'error');
                return isLiked;
            }
            
            // Use the first available username
            const firstUser = allUsersCheck.ok.answers[0];
            const validUsername = firstUser.data.username?.value || firstUser.data.username;
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
