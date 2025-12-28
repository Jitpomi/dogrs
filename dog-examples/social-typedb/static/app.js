// API Base URL
const API_BASE = 'http://127.0.0.1:3036';

// Current user data - will be populated from backend
let currentUser = {
    name: 'Jason Clark',
    username: 'user_2025_17', 
    title: 'Software Engineer',
    location: 'San Francisco, CA',
    initials: 'JC'
};

// Load user profile data from backend
async function loadUserProfile() {
    try {
        const userData = await makeQuery('persons', 'match $me isa person, has name "Jason Clark", has username $username; $employment (employee: $me, employer: $company) isa employment, has description $title; $company has name $company_name; select $me, $username, $title, $company_name;');
        
        if (userData.ok && userData.ok.answers && userData.ok.answers.length > 0) {
            const userInfo = userData.ok.answers[0].data;
            currentUser.username = userInfo.username?.value || currentUser.username;
            currentUser.title = userInfo.title?.value || currentUser.title;
            
            // Update UI elements with real data
            updateUserProfileUI();
        }
    } catch (error) {
        console.error('Error loading user profile:', error);
        // Keep default values if backend fails
    }
}

// Update UI elements with current user data
function updateUserProfileUI() {
    // Update navigation profile
    const navUserName = document.getElementById('navUserName');
    const navUserTitle = document.getElementById('navUserTitle');
    const navUserInitials = document.getElementById('navUserInitials');
    
    if (navUserName) navUserName.textContent = currentUser.name;
    if (navUserTitle) navUserTitle.textContent = currentUser.title;
    if (navUserInitials) navUserInitials.textContent = currentUser.initials;
    
    // Update profile card
    const profileName = document.getElementById('profileName');
    const profileTitle = document.getElementById('profileTitle');
    const profileLocation = document.getElementById('profileLocation');
    const profileInitials = document.getElementById('profileInitials');
    
    if (profileName) profileName.textContent = currentUser.name;
    if (profileTitle) profileTitle.textContent = currentUser.title;
    if (profileLocation) profileLocation.textContent = currentUser.location;
    if (profileInitials) profileInitials.textContent = currentUser.initials;
}

// UI Elements
const loading = document.getElementById('loading');
const modal = document.getElementById('modal');
const modalContent = document.getElementById('modalContent');
const feedContent = document.getElementById('feedContent');
const suggestedConnections = document.getElementById('suggestedConnections');
const connectionCount = document.getElementById('connectionCount');
const trendingTopics = document.getElementById('trendingTopics');

// Current view state
let currentView = 'feed';

// Show/Hide UI Elements
function showLoading() {
    loading.classList.add('active');
    loading.classList.remove('hidden');
}

function hideLoading() {
    loading.classList.remove('active');
    loading.classList.add('hidden');
}

function showModal(content) {
    modalContent.innerHTML = content;
    modal.classList.remove('hidden');
    modal.classList.add('flex');
}

function hideModal() {
    modal.classList.add('hidden');
    modal.classList.remove('flex');
}

// Navigation functions
function showFeed() {
    setActiveNav('feed');
    currentView = 'feed';
    loadFeed();
}

function showNetwork() {
    setActiveNav('network');
    currentView = 'network';
    loadNetwork();
}

function showJobs() {
    setActiveNav('jobs');
    currentView = 'jobs';
    loadJobs();
}

function showAnalytics() {
    setActiveNav('analytics');
    currentView = 'analytics';
    loadAnalytics();
}

function setActiveNav(activeItem) {
    document.querySelectorAll('.nav-item').forEach(item => {
        item.classList.remove('active');
    });
    
    // Only try to set active if the navigation element exists
    const navElement = document.querySelector(`[onclick="show${activeItem.charAt(0).toUpperCase() + activeItem.slice(1)}()"]`);
    if (navElement) {
        navElement.classList.add('active');
    }
}

// Social Media Functions
async function loadFeed() {
    showLoading();
    try {
        // Load posts from the database with like information
        const postsData = await makeQuery('posts', 'match $post isa post, has post-text $text, has post-id $id, has creation-timestamp $time; $posting (author: $author, page: $page, post: $post) isa posting; $author has name $name; select $post, $text, $id, $time, $name; sort $time desc; limit 10;');
        
        let feedHTML = '';
        let uniquePosts = [];
        
        if (postsData.ok && postsData.ok.answers && postsData.ok.answers.length > 0) {
            // Remove duplicates based on author name and post text combination
            const seenPosts = new Set();
            
            postsData.ok.answers.forEach(answer => {
                const text = String(answer.data.text?.value || answer.data.text || 'No content');
                const authorName = String(answer.data.name?.value || answer.data.name || 'Unknown Author');
                const postKey = `${authorName}:${text}`;
                
                if (!seenPosts.has(postKey)) {
                    seenPosts.add(postKey);
                    uniquePosts.push(answer);
                }
            });
            
            feedHTML = await Promise.all(uniquePosts.map(async (answer) => {
                try {
                    const text = String(answer.data.text?.value || answer.data.text || 'No content');
                    const authorName = String(answer.data.name?.value || answer.data.name || 'Unknown Author');
                    const timestamp = answer.data.time?.value || answer.data.time || new Date().toISOString();
                    const timeAgo = getTimeAgo(timestamp);
                    const initials = authorName.split(' ').map(n => n[0]).join('').toUpperCase();
                    const postId = answer.data.id?.value || answer.data.id || `post_${Date.now()}_${Math.random()}`;
                    
                    console.log('Processing post:', { authorName, postId, text: text.substring(0, 50) + '...' });
                    
                    // Get like count and user's like status for this post
                    const { likeCount, isLiked } = await getPostLikeInfo(postId);
                    
                    // Track view for this post (non-blocking)
                    trackPostView(postId);
                    
                    // Get view count for this post
                    const viewCount = await getPostViewCount(postId);
                    
                    // Get comment count for this post
                    const commentCount = await getPostCommentCount(postId);
                    
                    // Check if this is the current user's post
                    const currentUser = await getCurrentUser();
                    const isOwnPost = currentUser && authorName === currentUser.name;
                    
                    const postCard = createPostCard(authorName, initials, text, timeAgo, postId, likeCount, isLiked, viewCount, commentCount, isOwnPost);
                    console.log('Created post card for:', authorName);
                    return postCard;
                } catch (error) {
                    console.error('Error processing post:', error, answer);
                    return ''; // Return empty string for failed posts
                }
            }));
            feedHTML = feedHTML.join('');
        } else {
            feedHTML = `
                <div class="post-card text-center py-16">
                    <div class="w-16 h-16 bg-blue-100 rounded-full flex items-center justify-center mx-auto mb-4">
                        <i class="fas fa-rss text-blue-600 text-2xl"></i>
                    </div>
                    <h3 class="text-heading-3 mb-2">No posts yet</h3>
                    <p class="text-body mb-6" style="color: var(--neutral-500);">Be the first to share something with your network!</p>
                    <button class="btn-primary">Create your first post</button>
                </div>
            `;
        }
        
        feedContent.innerHTML = feedHTML;
        
        // Add event listeners for like buttons
        const likeButtons = document.querySelectorAll('.like-btn');
        console.log('Found like buttons:', likeButtons.length);
        likeButtons.forEach(button => {
            button.addEventListener('click', async function() {
                console.log('=== LIKE BUTTON CLICKED ===');
                const postId = this.getAttribute('data-post-id');
                console.log('Button post ID:', postId);
                
                // Disable button during request
                this.disabled = true;
                
                try {
                    const newLikeStatus = await togglePostLike(postId);
                    console.log('Toggle result:', newLikeStatus);
                    const { likeCount } = await getPostLikeInfo(postId);
                    console.log('New like count:', likeCount);
                    updateLikeButton(this, newLikeStatus, likeCount);
                    console.log('UI updated');
                } catch (error) {
                    console.error('Error handling like click:', error);
                } finally {
                    this.disabled = false;
                }
            });
        });

        // Add event listeners for comment buttons
        const commentButtons = document.querySelectorAll('.comment-btn');
        console.log('Found comment buttons:', commentButtons.length);
        commentButtons.forEach(button => {
            button.addEventListener('click', async function() {
                const postId = this.getAttribute('data-post-id');
                const commentSection = document.querySelector(`.comment-section[data-post-id="${postId}"]`);
                
                if (commentSection.classList.contains('hidden')) {
                    // Show comment section and load comments
                    commentSection.classList.remove('hidden');
                    await loadCommentsForPost(postId);
                } else {
                    // Hide comment section
                    commentSection.classList.add('hidden');
                }
            });
        });

        // Add event listeners for submit comment buttons
        const submitCommentButtons = document.querySelectorAll('.submit-comment-btn');
        console.log('Found submit comment buttons:', submitCommentButtons.length);
        submitCommentButtons.forEach(button => {
            button.addEventListener('click', async function() {
                const postId = this.getAttribute('data-post-id');
                const commentInput = document.querySelector(`.comment-input[data-post-id="${postId}"]`);
                const commentText = commentInput.value.trim();
                
                if (!commentText) {
                    showNotification('Please enter a comment', 'error');
                    return;
                }

                // Disable button during request
                this.disabled = true;
                this.textContent = 'Posting...';

                try {
                    const success = await addComment(postId, commentText);
                    if (success) {
                        commentInput.value = '';
                        await loadCommentsForPost(postId);
                        
                        // Update comment count display
                        const newCommentCount = await getPostCommentCount(postId);
                        updateCommentButton(postId, newCommentCount);
                    }
                } catch (error) {
                    console.error('Error submitting comment:', error);
                } finally {
                    this.disabled = false;
                    this.textContent = 'Post Comment';
                }
            });
        });

        // Add event listener for post submission
        const submitPostBtn = document.getElementById('submitPostBtn');
        const postTextarea = document.getElementById('postTextarea');
        const charCount = document.getElementById('charCount');
        
        // Add character counter functionality
        if (postTextarea && charCount) {
            postTextarea.addEventListener('input', function() {
                const currentLength = this.value.length;
                charCount.textContent = currentLength;
                
                // Change color when approaching limit
                if (currentLength > 450) {
                    charCount.style.color = 'var(--error-red)';
                } else if (currentLength > 400) {
                    charCount.style.color = 'var(--warning-amber)';
                } else {
                    charCount.style.color = 'var(--neutral-400)';
                }
            });
        }
        
        if (submitPostBtn && postTextarea) {
            submitPostBtn.addEventListener('click', async function() {
                const postText = postTextarea.value.trim();
                
                if (!postText) {
                    showNotification('Please enter some text for your post', 'error');
                    return;
                }

                if (postText.length > 500) {
                    showNotification('Post text is too long (max 500 characters)', 'error');
                    return;
                }

                // Disable button during request
                this.disabled = true;
                this.textContent = 'Posting...';

                try {
                    const result = await createPost(postText);
                    if (result.success) {
                        postTextarea.value = '';
                        // Refresh the feed to show the new post
                        await loadFeed();
                    }
                } catch (error) {
                    console.error('Error submitting post:', error);
                    showNotification('Error creating post', 'error');
                } finally {
                    this.disabled = false;
                    this.textContent = 'Post';
                }
            });
        }

        // Add event listeners for edit post buttons
        const editPostButtons = document.querySelectorAll('.edit-post-btn');
        console.log('Found edit post buttons:', editPostButtons.length);
        editPostButtons.forEach(button => {
            button.addEventListener('click', async function() {
                const postId = this.getAttribute('data-post-id');
                const postCard = this.closest('.post-card');
                const contentElement = postCard.querySelector('p');
                const currentText = contentElement.textContent;
                
                // Create inline edit form
                const editForm = document.createElement('div');
                editForm.className = 'edit-form mt-2';
                editForm.innerHTML = `
                    <textarea class="edit-textarea w-full p-3 border border-neutral-200 rounded-lg resize-none focus:outline-none focus:ring-2 focus:ring-blue-500" 
                              rows="3" maxlength="500">${currentText}</textarea>
                    <div class="flex justify-end space-x-2 mt-2">
                        <button class="cancel-edit-btn btn-secondary-sm">Cancel</button>
                        <button class="save-edit-btn btn-primary-sm">Save</button>
                    </div>
                `;
                
                // Hide original content and show edit form
                contentElement.style.display = 'none';
                contentElement.parentNode.insertBefore(editForm, contentElement.nextSibling);
                
                // Add event listeners for edit form buttons
                const cancelBtn = editForm.querySelector('.cancel-edit-btn');
                const saveBtn = editForm.querySelector('.save-edit-btn');
                const textarea = editForm.querySelector('.edit-textarea');
                
                cancelBtn.addEventListener('click', () => {
                    editForm.remove();
                    contentElement.style.display = 'block';
                });
                
                saveBtn.addEventListener('click', async () => {
                    const newText = textarea.value.trim();
                    if (!newText) {
                        showNotification('Post text cannot be empty', 'error');
                        return;
                    }
                    
                    saveBtn.disabled = true;
                    saveBtn.textContent = 'Saving...';
                    
                    try {
                        const success = await editPost(postId, newText);
                        if (success) {
                            contentElement.textContent = newText;
                            editForm.remove();
                            contentElement.style.display = 'block';
                        }
                    } catch (error) {
                        console.error('Error editing post:', error);
                    } finally {
                        saveBtn.disabled = false;
                        saveBtn.textContent = 'Save';
                    }
                });
            });
        });

        // Add event listeners for delete post buttons
        const deletePostButtons = document.querySelectorAll('.delete-post-btn');
        console.log('Found delete post buttons:', deletePostButtons.length);
        deletePostButtons.forEach(button => {
            button.addEventListener('click', async function() {
                const postId = this.getAttribute('data-post-id');
                
                // Show confirmation dialog
                if (confirm('Are you sure you want to delete this post? This action cannot be undone.')) {
                    this.disabled = true;
                    
                    try {
                        const success = await deletePost(postId);
                        if (success) {
                            // Remove post card from DOM and refresh feed
                            await loadFeed();
                        }
                    } catch (error) {
                        console.error('Error deleting post:', error);
                    } finally {
                        this.disabled = false;
                    }
                }
            });
        });
        
        // Initialize user switching functionality
        await initializeUserSwitching();
        
        // Load suggested connections and trending topics
        await loadSuggestedConnections();
        await loadTrendingTopics();
        
        // Update post count with all posts displayed in feed
        const postCountElement = document.getElementById('postCount');
        if (postCountElement) {
            const currentUser = await getCurrentUser();
            const currentUserPosts = uniquePosts.filter(answer => {
                const authorName = String(answer.data.name?.value || answer.data.name || 'Unknown Author');
                return currentUser && authorName === currentUser.name;
            });
            postCountElement.textContent = currentUserPosts.length;
        }
        
    } catch (error) {
        console.error('Error loading feed:', error);
        feedContent.innerHTML = `
            <div class="post-card text-center py-16">
                <div class="w-16 h-16 bg-red-100 rounded-full flex items-center justify-center mx-auto mb-4">
                    <i class="fas fa-exclamation-triangle text-red-600 text-2xl"></i>
                </div>
                <h3 class="text-heading-3 mb-2">Unable to load posts</h3>
                <p class="text-body mb-6" style="color: var(--neutral-500);">Please check your connection and try again</p>
                <button class="btn-primary" onclick="loadFeed()">Retry</button>
            </div>
        `;
    }
    hideLoading();
}

function createPostCard(authorName, authorInitials, content, timeAgo, postId, likeCount = 0, isLiked = false, viewCount = 0, commentCount = 0, isOwnPost = false) {
    const likeIcon = isLiked ? 'fas fa-thumbs-up' : 'far fa-thumbs-up';
    const likeColor = isLiked ? 'text-blue-600' : 'text-neutral-500 hover:text-blue-600';
    const likeText = likeCount > 0 ? `${likeCount}` : '';
    
    return `
        <div class="post-card card-hover p-6 mb-4" data-post-id="${postId}">
            <div class="flex items-start space-x-4">
                <div class="avatar">${authorInitials}</div>
                <div class="flex-1">
                    <div class="flex items-center justify-between mb-3">
                        <div class="flex items-center space-x-2">
                            <h4 class="text-body-large font-semibold" style="color: var(--neutral-900);">${authorName}</h4>
                            <span class="text-caption" style="color: var(--neutral-400);">•</span>
                            <span class="text-caption" style="color: var(--neutral-500);">${timeAgo}</span>
                        </div>
                        ${isOwnPost ? `
                        <div class="flex items-center space-x-2">
                            <button class="edit-post-btn text-neutral-400 hover:text-blue-600 transition-colors duration-150 p-1" data-post-id="${postId}" title="Edit post">
                                <i class="fas fa-edit text-sm"></i>
                            </button>
                            <button class="delete-post-btn text-neutral-400 hover:text-red-600 transition-colors duration-150 p-1" data-post-id="${postId}" title="Delete post">
                                <i class="fas fa-trash text-sm"></i>
                            </button>
                        </div>
                        ` : ''}
                    </div>
                    <p class="text-body mb-4" style="color: var(--neutral-700); line-height: 1.6;">${content}</p>
                    <div class="flex items-center justify-between pt-3 border-t border-neutral-100">
                        <div class="flex items-center space-x-6">
                            <button class="like-btn flex items-center space-x-2 ${likeColor} transition-colors duration-150 p-2 rounded-lg hover:bg-blue-50" data-post-id="${postId}" data-author="${authorName}">
                                <i class="${likeIcon} text-lg"></i>
                                <span class="text-body font-medium">Like</span>
                                ${likeText ? `<span class="like-count text-sm">${likeText}</span>` : ''}
                            </button>
                            <button class="comment-btn flex items-center space-x-2 text-neutral-500 hover:text-green-600 transition-colors duration-150 p-2 rounded-lg hover:bg-green-50" data-post-id="${postId}">
                                <i class="far fa-comment text-lg"></i>
                                <span class="text-body font-medium">Comment</span>
                                ${commentCount > 0 ? `<span class="comment-count text-sm">${commentCount}</span>` : ''}
                            </button>
                            <button class="flex items-center space-x-2 text-neutral-500 hover:text-purple-600 transition-colors duration-150 p-2 rounded-lg hover:bg-purple-50">
                                <i class="fas fa-share text-lg"></i>
                                <span class="text-body font-medium">Share</span>
                            </button>
                        </div>
                        <div class="flex items-center space-x-2 text-caption" style="color: var(--neutral-400);">
                            <i class="fas fa-eye"></i>
                            <span>${viewCount} views</span>
                        </div>
                    </div>
                    
                    <!-- Comment Section -->
                    <div class="comment-section mt-4 hidden" data-post-id="${postId}">
                        <div class="border-t border-neutral-100 pt-4">
                            <!-- Comment Input -->
                            <div class="flex items-start space-x-3 mb-4">
                                <div class="avatar-sm">U</div>
                                <div class="flex-1">
                                    <textarea class="comment-input w-full p-3 border border-neutral-200 rounded-lg resize-none focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent" 
                                              rows="2" 
                                              placeholder="Write a comment..." 
                                              data-post-id="${postId}"></textarea>
                                    <div class="flex justify-end mt-2">
                                        <button class="submit-comment-btn btn-primary-sm" data-post-id="${postId}">Post Comment</button>
                                    </div>
                                </div>
                            </div>
                            
                            <!-- Comments List -->
                            <div class="comments-list" data-post-id="${postId}">
                                <!-- Comments will be loaded here -->
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    `;
}

// Load and display comments for a specific post
async function loadCommentsForPost(postId) {
    try {
        const commentsList = document.querySelector(`.comments-list[data-post-id="${postId}"]`);
        if (!commentsList) return;

        // Show loading state
        commentsList.innerHTML = '<div class="text-center py-2 text-neutral-500">Loading comments...</div>';

        // Get comments from database
        const comments = await getPostComments(postId);

        if (comments.length === 0) {
            commentsList.innerHTML = '<div class="text-center py-4 text-neutral-500">No comments yet. Be the first to comment!</div>';
            return;
        }

        // Generate comments HTML
        const currentUser = await getCurrentUser();
        const commentsHTML = comments.map(comment => {
            const initials = comment.authorName.split(' ').map(n => n[0]).join('').toUpperCase();
            const timeAgo = getTimeAgo(comment.timestamp);
            const isOwnComment = currentUser && comment.authorName === currentUser.name;
            
            return `
                <div class="comment-item flex items-start space-x-3 mb-3 pb-3 border-b border-neutral-100 last:border-b-0" data-comment-id="${comment.id}">
                    <div class="avatar-sm">${initials}</div>
                    <div class="flex-1">
                        <div class="flex items-center justify-between mb-1">
                            <div class="flex items-center space-x-2">
                                <span class="font-semibold text-sm" style="color: var(--neutral-900);">${comment.authorName}</span>
                                <span class="text-xs" style="color: var(--neutral-400);">${timeAgo}</span>
                            </div>
                            ${isOwnComment ? `
                            <div class="flex items-center space-x-1">
                                <button class="edit-comment-btn text-neutral-400 hover:text-blue-600 transition-colors duration-150 p-1" data-comment-id="${comment.id}" title="Edit comment">
                                    <i class="fas fa-edit text-xs"></i>
                                </button>
                                <button class="delete-comment-btn text-neutral-400 hover:text-red-600 transition-colors duration-150 p-1" data-comment-id="${comment.id}" title="Delete comment">
                                    <i class="fas fa-trash text-xs"></i>
                                </button>
                            </div>
                            ` : ''}
                        </div>
                        <p class="comment-text text-sm" style="color: var(--neutral-700); line-height: 1.4;">${comment.text}</p>
                    </div>
                </div>
            `;
        }).join('');

        commentsList.innerHTML = commentsHTML;
        
        // Add event listeners for comment edit and delete buttons
        addCommentEditDeleteListeners(postId);
    } catch (error) {
        console.error('Error loading comments:', error);
        const commentsList = document.querySelector(`.comments-list[data-post-id="${postId}"]`);
        if (commentsList) {
            commentsList.innerHTML = '<div class="text-center py-2 text-red-500">Failed to load comments</div>';
        }
    }
}

// Add event listeners for comment edit and delete buttons
function addCommentEditDeleteListeners(postId) {
    const commentsList = document.querySelector(`.comments-list[data-post-id="${postId}"]`);
    if (!commentsList) return;
    
    // Edit comment buttons
    const editCommentButtons = commentsList.querySelectorAll('.edit-comment-btn');
    editCommentButtons.forEach(button => {
        button.addEventListener('click', async function() {
            const commentId = this.getAttribute('data-comment-id');
            const commentItem = this.closest('.comment-item');
            const commentTextElement = commentItem.querySelector('.comment-text');
            const currentText = commentTextElement.textContent;
            
            // Create inline edit form
            const editForm = document.createElement('div');
            editForm.className = 'edit-comment-form mt-1';
            editForm.innerHTML = `
                <textarea class="edit-comment-textarea w-full p-2 border border-neutral-200 rounded text-sm resize-none focus:outline-none focus:ring-2 focus:ring-blue-500" 
                          rows="2" maxlength="500">${currentText}</textarea>
                <div class="flex justify-end space-x-2 mt-1">
                    <button class="cancel-comment-edit-btn text-xs px-2 py-1 text-neutral-600 hover:text-neutral-800">Cancel</button>
                    <button class="save-comment-edit-btn text-xs px-2 py-1 bg-blue-600 text-white rounded hover:bg-blue-700">Save</button>
                </div>
            `;
            
            // Hide original text and show edit form
            commentTextElement.style.display = 'none';
            commentTextElement.parentNode.insertBefore(editForm, commentTextElement.nextSibling);
            
            // Add event listeners for edit form buttons
            const cancelBtn = editForm.querySelector('.cancel-comment-edit-btn');
            const saveBtn = editForm.querySelector('.save-comment-edit-btn');
            const textarea = editForm.querySelector('.edit-comment-textarea');
            
            cancelBtn.addEventListener('click', () => {
                editForm.remove();
                commentTextElement.style.display = 'block';
            });
            
            saveBtn.addEventListener('click', async () => {
                const newText = textarea.value.trim();
                if (!newText) {
                    showNotification('Comment text cannot be empty', 'error');
                    return;
                }
                
                saveBtn.disabled = true;
                saveBtn.textContent = 'Saving...';
                
                try {
                    const success = await editComment(commentId, newText);
                    if (success) {
                        commentTextElement.textContent = newText;
                        editForm.remove();
                        commentTextElement.style.display = 'block';
                    }
                } catch (error) {
                    console.error('Error editing comment:', error);
                } finally {
                    saveBtn.disabled = false;
                    saveBtn.textContent = 'Save';
                }
            });
        });
    });
    
    // Delete comment buttons
    const deleteCommentButtons = commentsList.querySelectorAll('.delete-comment-btn');
    deleteCommentButtons.forEach(button => {
        button.addEventListener('click', async function() {
            const commentId = this.getAttribute('data-comment-id');
            
            // Show confirmation dialog
            if (confirm('Are you sure you want to delete this comment? This action cannot be undone.')) {
                this.disabled = true;
                
                try {
                    const success = await deleteComment(commentId);
                    if (success) {
                        // Refresh comments and update count
                        await loadCommentsForPost(postId);
                        const newCommentCount = await getPostCommentCount(postId);
                        updateCommentButton(postId, newCommentCount);
                    }
                } catch (error) {
                    console.error('Error deleting comment:', error);
                } finally {
                    this.disabled = false;
                }
            }
        });
    });
}

// Initialize user switching functionality
async function initializeUserSwitching() {
    try {
        // Update profile display with current user
        await updateUserProfile();
        
        // Load all users for dropdown
        await loadUserSwitchDropdown();
        
        // Add event listeners for dropdown
        const profileDropdownBtn = document.getElementById('profileDropdownBtn');
        const userSwitchDropdown = document.getElementById('userSwitchDropdown');
        
        if (profileDropdownBtn && userSwitchDropdown) {
            profileDropdownBtn.addEventListener('click', (e) => {
                e.stopPropagation();
                userSwitchDropdown.classList.toggle('hidden');
            });
            
            // Close dropdown when clicking outside
            document.addEventListener('click', (e) => {
                if (!profileDropdownBtn.contains(e.target) && !userSwitchDropdown.contains(e.target)) {
                    userSwitchDropdown.classList.add('hidden');
                }
            });
        }
    } catch (error) {
        console.error('Error initializing user switching:', error);
    }
}

// Update user profile display
async function updateUserProfile() {
    try {
        const currentUser = await getCurrentUser();
        if (!currentUser) return;
        
        // Update navigation profile
        const navUserName = document.getElementById('navUserName');
        const navUserInitials = document.getElementById('navUserInitials');
        
        if (navUserName) {
            navUserName.textContent = currentUser.name;
        }
        
        if (navUserInitials) {
            const initials = currentUser.name.split(' ').map(n => n[0]).join('').toUpperCase();
            navUserInitials.textContent = initials;
        }
        
        // Update post composer placeholder
        const postTextarea = document.getElementById('postTextarea');
        if (postTextarea) {
            const firstName = currentUser.name.split(' ')[0];
            postTextarea.placeholder = `What's on your mind, ${firstName}?`;
        }
        
        // Update sidebar profile if it exists
        const profileName = document.querySelector('.profile-card h3');
        if (profileName) {
            profileName.textContent = currentUser.name;
        }
        
    } catch (error) {
        console.error('Error updating user profile:', error);
    }
}

// Load user switching dropdown with infinite scroll
async function loadUserSwitchDropdown() {
    try {
        const currentUser = await getCurrentUser();
        const usersList = document.getElementById('usersList');
        
        if (!usersList) return;
        
        // Initialize pagination state
        window.userPagination = {
            offset: 0,
            limit: 20,
            loading: false,
            hasMore: true,
            allUsers: [],
            filteredUsers: [],
            isSearching: false
        };
        
        window.currentUser = currentUser;
        
        // Load initial batch of users
        await loadMoreUsers();
        
        // Load recent users
        await loadRecentUsers();
        
        // Add search functionality
        setupUserSearch();
        
        // Setup infinite scroll
        setupInfiniteScroll();
        
    } catch (error) {
        console.error('Error loading user switch dropdown:', error);
    }
}

// Load more users for infinite scroll
async function loadMoreUsers() {
    if (window.userPagination.loading || !window.userPagination.hasMore) return;
    
    window.userPagination.loading = true;
    showLoadingIndicator();
    
    try {
        const newUsers = await getAllUsers(window.userPagination.limit, window.userPagination.offset);
        
        if (newUsers.length === 0) {
            window.userPagination.hasMore = false;
            showNoMoreUsersIndicator();
        } else {
            window.userPagination.allUsers = [...window.userPagination.allUsers, ...newUsers];
            window.userPagination.offset += newUsers.length;
            
            // If not searching, append to display
            if (!window.userPagination.isSearching) {
                appendUsers(newUsers, window.currentUser);
            }
        }
        
    } catch (error) {
        console.error('Error loading more users:', error);
    } finally {
        window.userPagination.loading = false;
        hideLoadingIndicator();
    }
}

// Setup infinite scroll for user list
function setupInfiniteScroll() {
    const usersList = document.getElementById('usersList');
    if (!usersList) return;
    
    usersList.addEventListener('scroll', () => {
        const { scrollTop, scrollHeight, clientHeight } = usersList;
        
        // Load more when scrolled to within 50px of bottom
        if (scrollTop + clientHeight >= scrollHeight - 50) {
            if (!window.userPagination.isSearching) {
                loadMoreUsers();
            }
        }
    });
}

// Display users in the dropdown (for search results)
function displayUsers(users, currentUser, isFiltered = false) {
    const usersList = document.getElementById('usersList');
    const noUsersFound = document.getElementById('noUsersFound');
    
    if (!users.length && isFiltered) {
        usersList.innerHTML = '';
        noUsersFound.classList.remove('hidden');
        return;
    }
    
    noUsersFound.classList.add('hidden');
    
    const usersHTML = users.map(user => {
        const initials = user.name.split(' ').map(n => n[0]).join('').toUpperCase();
        const isCurrentUser = currentUser && user.username === currentUser.username;
        
        return `
            <div class="user-option flex items-center p-2 hover:bg-neutral-50 cursor-pointer transition-colors duration-150 ${isCurrentUser ? 'bg-blue-50' : ''}" 
                 data-username="${user.username}" data-name="${user.name}">
                <div class="avatar-sm mr-3 text-xs">${initials}</div>
                <div class="flex-1">
                    <div class="text-sm font-semibold text-neutral-800">${user.name}</div>
                    <div class="text-xs text-neutral-500">@${user.username}</div>
                </div>
                ${isCurrentUser ? '<i class="fas fa-check text-blue-600 text-sm"></i>' : ''}
            </div>
        `;
    }).join('');
    
    usersList.innerHTML = usersHTML;
    addUserClickListeners();
}

// Append users to the dropdown (for infinite scroll)
function appendUsers(users, currentUser) {
    const usersList = document.getElementById('usersList');
    if (!usersList || !users.length) return;
    
    const usersHTML = users.map(user => {
        const initials = user.name.split(' ').map(n => n[0]).join('').toUpperCase();
        const isCurrentUser = currentUser && user.username === currentUser.username;
        
        return `
            <div class="user-option flex items-center p-2 hover:bg-neutral-50 cursor-pointer transition-colors duration-150 ${isCurrentUser ? 'bg-blue-50' : ''}" 
                 data-username="${user.username}" data-name="${user.name}">
                <div class="avatar-sm mr-3 text-xs">${initials}</div>
                <div class="flex-1">
                    <div class="text-sm font-semibold text-neutral-800">${user.name}</div>
                    <div class="text-xs text-neutral-500">@${user.username}</div>
                </div>
                ${isCurrentUser ? '<i class="fas fa-check text-blue-600 text-sm"></i>' : ''}
            </div>
        `;
    }).join('');
    
    usersList.insertAdjacentHTML('beforeend', usersHTML);
    addUserClickListeners();
}

// Add click listeners to user options
function addUserClickListeners() {
    const userOptions = document.querySelectorAll('#usersList .user-option');
    userOptions.forEach(option => {
        // Remove existing listeners to prevent duplicates
        option.replaceWith(option.cloneNode(true));
    });
    
    // Re-select after cloning
    const newUserOptions = document.querySelectorAll('#usersList .user-option');
    newUserOptions.forEach(option => {
        option.addEventListener('click', async (e) => {
            const username = option.getAttribute('data-username');
            const dropdown = document.getElementById('userSwitchDropdown');
            
            // Save to recent users
            saveRecentUser(username);
            
            // Close dropdown
            dropdown.classList.add('hidden');
            
            // Clear search
            const searchInput = document.getElementById('userSearchInput');
            if (searchInput) searchInput.value = '';
            
            // Switch user
            await switchUser(username);
            
            // Reload dropdown to update current user indicator
            await loadUserSwitchDropdown();
        });
    });
}

// Setup user search functionality
function setupUserSearch() {
    const searchInput = document.getElementById('userSearchInput');
    if (!searchInput) return;
    
    searchInput.addEventListener('input', (e) => {
        const searchTerm = e.target.value.toLowerCase().trim();
        
        if (!searchTerm) {
            // Exit search mode and show paginated users
            window.userPagination.isSearching = false;
            displayUsers(window.userPagination.allUsers, window.currentUser);
            hideAllIndicators();
            return;
        }
        
        // Enter search mode
        window.userPagination.isSearching = true;
        hideAllIndicators();
        
        // Filter users based on search term from loaded users
        const filteredUsers = window.userPagination.allUsers.filter(user => 
            user.name.toLowerCase().includes(searchTerm) ||
            user.username.toLowerCase().includes(searchTerm)
        );
        
        displayUsers(filteredUsers, window.currentUser, true);
    });
    
    // Clear search when dropdown opens
    searchInput.addEventListener('focus', () => {
        searchInput.select();
    });
}

// Load recent users
async function loadRecentUsers() {
    try {
        const recentUsernames = JSON.parse(localStorage.getItem('recentUsers') || '[]');
        const recentUsersSection = document.getElementById('recentUsersSection');
        const recentUsersList = document.getElementById('recentUsersList');
        
        if (!recentUsernames.length || !window.allUsers) {
            recentUsersSection.classList.add('hidden');
            return;
        }
        
        // Get recent user objects
        const recentUsers = recentUsernames
            .map(username => window.allUsers.find(user => user.username === username))
            .filter(user => user && user.username !== window.currentUser?.username)
            .slice(0, 3); // Show max 3 recent users
        
        if (!recentUsers.length) {
            recentUsersSection.classList.add('hidden');
            return;
        }
        
        recentUsersSection.classList.remove('hidden');
        
        const recentHTML = recentUsers.map(user => {
            const initials = user.name.split(' ').map(n => n[0]).join('').toUpperCase();
            
            return `
                <div class="user-option flex items-center p-2 hover:bg-neutral-50 cursor-pointer transition-colors duration-150" 
                     data-username="${user.username}" data-name="${user.name}">
                    <div class="avatar-sm mr-3 text-xs">${initials}</div>
                    <div class="flex-1">
                        <div class="text-sm font-semibold text-neutral-800">${user.name}</div>
                        <div class="text-xs text-neutral-500">@${user.username}</div>
                    </div>
                </div>
            `;
        }).join('');
        
        recentUsersList.innerHTML = recentHTML;
        
        // Add click listeners to recent user options
        const recentOptions = recentUsersList.querySelectorAll('.user-option');
        recentOptions.forEach(option => {
            option.addEventListener('click', async (e) => {
                const username = option.getAttribute('data-username');
                const dropdown = document.getElementById('userSwitchDropdown');
                
                // Move to top of recent users
                saveRecentUser(username);
                
                // Close dropdown
                dropdown.classList.add('hidden');
                
                // Clear search
                const searchInput = document.getElementById('userSearchInput');
                if (searchInput) searchInput.value = '';
                
                // Switch user
                await switchUser(username);
                
                // Reload dropdown
                await loadUserSwitchDropdown();
            });
        });
        
    } catch (error) {
        console.error('Error loading recent users:', error);
    }
}

// Save user to recent users list
function saveRecentUser(username) {
    try {
        let recentUsers = JSON.parse(localStorage.getItem('recentUsers') || '[]');
        
        // Remove if already exists
        recentUsers = recentUsers.filter(u => u !== username);
        
        // Add to beginning
        recentUsers.unshift(username);
        
        // Keep only last 5
        recentUsers = recentUsers.slice(0, 5);
        
        localStorage.setItem('recentUsers', JSON.stringify(recentUsers));
    } catch (error) {
        console.error('Error saving recent user:', error);
    }
}

// Loading indicator functions
function showLoadingIndicator() {
    const loadingIndicator = document.getElementById('loadingMoreUsers');
    if (loadingIndicator) {
        loadingIndicator.classList.remove('hidden');
    }
}

function hideLoadingIndicator() {
    const loadingIndicator = document.getElementById('loadingMoreUsers');
    if (loadingIndicator) {
        loadingIndicator.classList.add('hidden');
    }
}

function showNoMoreUsersIndicator() {
    const noMoreIndicator = document.getElementById('noMoreUsers');
    if (noMoreIndicator) {
        noMoreIndicator.classList.remove('hidden');
    }
}

function hideAllIndicators() {
    const loadingIndicator = document.getElementById('loadingMoreUsers');
    const noMoreIndicator = document.getElementById('noMoreUsers');
    const noUsersFound = document.getElementById('noUsersFound');
    
    if (loadingIndicator) loadingIndicator.classList.add('hidden');
    if (noMoreIndicator) noMoreIndicator.classList.add('hidden');
    if (noUsersFound) noUsersFound.classList.add('hidden');
}

function getSamplePosts() {
    const samplePosts = [
        {
            author: 'Mia Lewis',
            initials: 'ML',
            content: 'Excited to share that our ML team at Google just launched a new feature! The power of graph databases in understanding user connections is incredible. #MachineLearning #Google',
            time: '2 hours ago'
        },
        {
            author: 'Alex Chen',
            initials: 'AC',
            content: 'Looking for talented engineers to join our team at Google. We\'re working on some amazing projects in the AI space. DM me if interested! #Hiring #AI #Google',
            time: '4 hours ago'
        },
        {
            author: 'John Smith',
            initials: 'JS',
            content: 'Just had an amazing coffee chat with a friend who works at Google. The tech industry is all about connections and relationships. #Networking #TechCareers',
            time: '6 hours ago'
        }
    ];
    
    return samplePosts.map(post => createPostCard(post.author, post.initials, post.content, post.time)).join('');
}

async function loadSuggestedConnections() {
    try {
        // Get current user dynamically
        const currentUser = await getCurrentUser();
        if (!currentUser) {
            console.error('No current user found for suggested connections');
            return;
        }
        
        // Find people connected through mutual friends
        const connectionsData = await makeQuery('persons', `match $me isa person, has name "${currentUser.name}"; $friend1 (friend: $me, friend: $mutual) isa friendship; $friend2 (friend: $mutual, friend: $suggestion) isa friendship; $suggestion has name $name; not { $direct (friend: $me, friend: $suggestion) isa friendship; }; not { $suggestion is $me; }; limit 5; select $suggestion, $name;`);
        
        console.log('Connections query response:', connectionsData);
        console.log('Query returned answers:', connectionsData.ok?.answers?.length || 0);
        
        let suggestionsHTML = '';
        
        if (connectionsData.ok && connectionsData.ok.answers && connectionsData.ok.answers.length > 0) {
            console.log('Using TypeDB query results for suggestions');
            // Remove duplicates based on ID
            const uniqueConnections = [];
            const seenIds = new Set();
            
            connectionsData.ok.answers.forEach(answer => {
                const personId = answer.data.suggestion?.iid || JSON.stringify(answer.data.suggestion);
                if (!seenIds.has(personId)) {
                    seenIds.add(personId);
                    uniqueConnections.push(answer);
                }
            });
            
            suggestionsHTML = uniqueConnections.map(answer => {
                const name = String(answer.data.name?.value || answer.data.name || 'Unknown User');
                const initials = name.split(' ').map(n => n[0]).join('').toUpperCase();
                
                return `
                    <div class="flex items-center justify-between p-3 rounded-lg hover:bg-neutral-50 transition-colors duration-150">
                        <div class="flex items-center space-x-3">
                            <div class="avatar avatar-sm">${initials}</div>
                            <div>
                                <div class="text-body font-semibold" style="color: var(--neutral-900);">${name}</div>
                                <div class="text-caption" style="color: var(--neutral-500);">Mutual connection</div>
                            </div>
                        </div>
                        <button class="btn-secondary connect-person-btn" style="padding: 8px 16px; font-size: 12px;" data-person="${name}">Connect</button>
                    </div>
                `;
            }).join('');
        } else {
            console.log('No TypeDB results, showing empty state');
            // Show empty state when no suggestions found
            suggestionsHTML = `
                <div class="text-center py-12">
                    <div class="w-16 h-16 bg-blue-100 rounded-full flex items-center justify-center mx-auto mb-4">
                        <i class="fas fa-users text-blue-600 text-2xl"></i>
                    </div>
                    <h3 class="text-heading-3 mb-2">No suggestions available</h3>
                    <p class="text-body" style="color: var(--neutral-500);">Check back later for new connection opportunities</p>
                </div>
            `;
        }
        
        suggestedConnections.innerHTML = suggestionsHTML;
        
        // Add event listeners to connect buttons
        const connectButtons = document.querySelectorAll('.connect-person-btn');
        connectButtons.forEach(button => {
            button.addEventListener('click', function() {
                const personName = this.getAttribute('data-person');
                connectPerson(personName);
            });
        });
        
        // Update connection count (count unique people by ID, not names)
        const friendsData = await makeQuery('persons', `match $me isa person, has name "${currentUser.name}"; $friendship (friend: $me, friend: $friend) isa friendship; $friend has name $name; select $friend, $name;`);
        let uniqueCount = 0;
        if (friendsData.ok && friendsData.ok.answers && friendsData.ok.answers.length > 0) {
            const uniqueFriends = new Set();
            friendsData.ok.answers.forEach(answer => {
                const friendId = answer.data.friend?.iid || JSON.stringify(answer.data.friend);
                uniqueFriends.add(friendId);
            });
            uniqueCount = uniqueFriends.size;
        }
        connectionCount.textContent = uniqueCount;
        
    } catch (error) {
        console.error('Error loading suggestions:', error);
        suggestedConnections.innerHTML = '<p class="text-center text-red-500 py-4">Error loading suggestions</p>';
    }
}


async function loadTrendingTopics() {
    try {
        // Query for posts with tags to get trending topics
        const tagsData = await makeQuery('posts', 'match $post isa post, has tag $tag; select $tag;');
        
        let trendingHTML = '';
        
        if (tagsData.ok && tagsData.ok.answers && tagsData.ok.answers.length > 0) {
            // Count tag occurrences
            const tagCounts = {};
            tagsData.ok.answers.forEach(answer => {
                const tag = String(answer.data.tag?.value || answer.data.tag || '');
                if (tag && tag.trim()) {
                    tagCounts[tag] = (tagCounts[tag] || 0) + 1;
                }
            });
            
            // Sort by count and take top 3
            const sortedTags = Object.entries(tagCounts)
                .sort(([,a], [,b]) => b - a)
                .slice(0, 3);
            
            if (sortedTags.length > 0) {
                trendingHTML = sortedTags.map(([tag, count]) => `
                    <div class="p-3 rounded-lg hover:bg-blue-50 transition-colors duration-150 cursor-pointer">
                        <div class="flex items-center justify-between">
                            <div>
                                <div class="text-body font-semibold" style="color: var(--primary-blue);">#${tag}</div>
                                <div class="text-caption" style="color: var(--neutral-500);">${count} post${count !== 1 ? 's' : ''}</div>
                            </div>
                            <i class="fas fa-arrow-up text-green-500 text-sm"></i>
                        </div>
                    </div>
                `).join('');
            } else {
                trendingHTML = `
                    <div class="text-center py-8">
                        <div class="w-12 h-12 bg-blue-100 rounded-full flex items-center justify-center mx-auto mb-3">
                            <i class="fas fa-hashtag text-blue-600"></i>
                        </div>
                        <p class="text-caption" style="color: var(--neutral-500);">No trending topics yet</p>
                    </div>
                `;
            }
        } else {
            // Show empty state if no tags found
            trendingHTML = `
                <div class="text-center py-8">
                    <div class="w-12 h-12 bg-blue-100 rounded-full flex items-center justify-center mx-auto mb-3">
                        <i class="fas fa-hashtag text-blue-600"></i>
                    </div>
                    <p class="text-caption" style="color: var(--neutral-500);">No trending topics yet</p>
                </div>
            `;
        }
        
        trendingTopics.innerHTML = trendingHTML;
        
    } catch (error) {
        console.error('Error loading trending topics:', error);
        trendingTopics.innerHTML = `
            <div class="text-center py-8">
                <div class="w-12 h-12 bg-red-100 rounded-full flex items-center justify-center mx-auto mb-3">
                    <i class="fas fa-exclamation-triangle text-red-600"></i>
                </div>
                <p class="text-caption" style="color: var(--neutral-500);">Unable to load trending topics</p>
            </div>
        `;
    }
}

function getSampleTrending() {
    return `
        <div class="p-3 rounded-lg hover:bg-blue-50 transition-colors duration-150 cursor-pointer">
            <div class="flex items-center justify-between">
                <div>
                    <div class="text-body font-semibold" style="color: var(--primary-blue);">#TechJobs</div>
                    <div class="text-caption" style="color: var(--neutral-500);">No posts yet</div>
                </div>
                <i class="fas fa-arrow-up text-green-500 text-sm"></i>
            </div>
        </div>
        <div class="p-3 rounded-lg hover:bg-green-50 transition-colors duration-150 cursor-pointer">
            <div class="flex items-center justify-between">
                <div>
                    <div class="text-body font-semibold" style="color: var(--success-green);">#RemoteWork</div>
                    <div class="text-caption" style="color: var(--neutral-500);">No posts yet</div>
                </div>
                <i class="fas fa-arrow-up text-green-500 text-sm"></i>
            </div>
        </div>
        <div class="p-3 rounded-lg hover:bg-purple-50 transition-colors duration-150 cursor-pointer">
            <div class="flex items-center justify-between">
                <div>
                    <div class="text-body font-semibold" style="color: #8b5cf6;">#AI</div>
                    <div class="text-caption" style="color: var(--neutral-500);">No posts yet</div>
                </div>
                <i class="fas fa-arrow-up text-green-500 text-sm"></i>
            </div>
        </div>
    `;
}

async function loadNetwork() {
    showLoading();
    try {
        // Get current user dynamically
        const currentUser = await getCurrentUser();
        if (!currentUser) {
            console.error('No current user found for network');
            hideLoading();
            return;
        }
        
        const networkData = await makeQuery('persons', `match $me isa person, has name "${currentUser.name}"; $network_relation_xyz (friend: $me, friend: $friend) isa friendship; $friend has name $name; select $friend, $name;`);
        
        let networkHTML = '<div class="space-y-4">';
        
        if (networkData.ok && networkData.ok.answers && networkData.ok.answers.length > 0) {
            // Remove duplicates based on ID (consistent with Analytics)
            const uniqueConnections = [];
            const seenIds = new Set();
            
            networkData.ok.answers.forEach(answer => {
                const friendId = answer.data.friend?.iid || JSON.stringify(answer.data.friend);
                if (!seenIds.has(friendId)) {
                    seenIds.add(friendId);
                    uniqueConnections.push(answer);
                }
            });
            
            networkHTML += uniqueConnections.map(answer => {
                const name = String(answer.data.name?.value || answer.data.name || 'Unknown');
                const initials = name.split(' ').map(n => n[0]).join('').toUpperCase();
                
                return `
                    <div class="profile-card p-4">
                        <div class="flex items-center space-x-4">
                            <div class="avatar">${initials}</div>
                            <div class="flex-1">
                                <h4 class="font-semibold text-gray-900">${name}</h4>
                                <p class="text-gray-600">Connected</p>
                                <p class="text-sm text-gray-500">View their profile and send messages</p>
                            </div>
                            <div class="flex space-x-2">
                                <button class="disconnect-person-btn px-3 py-1 bg-red-100 text-red-700 rounded-full text-sm hover:bg-red-200" data-person="${name}">Disconnect</button>
                                <button class="view-profile-btn px-3 py-1 border border-gray-300 text-gray-700 rounded-full text-sm hover:bg-gray-50" data-person="${name}">View Profile</button>
                            </div>
                        </div>
                    </div>
                `;
            }).join('');
        } else {
            networkHTML += '<p class="text-center text-gray-500 py-8">No connections found</p>';
        }
        
        networkHTML += '</div>';
        feedContent.innerHTML = networkHTML;
        
        // Add event listeners to disconnect buttons
        const disconnectButtons = document.querySelectorAll('.disconnect-person-btn');
        disconnectButtons.forEach(button => {
            button.addEventListener('click', function() {
                const personName = this.getAttribute('data-person');
                disconnectPerson(personName);
            });
        });
        
        // Add event listeners to view profile buttons
        const viewProfileButtons = document.querySelectorAll('.view-profile-btn');
        console.log('Found view profile buttons in Network:', viewProfileButtons.length);
        viewProfileButtons.forEach(button => {
            console.log('Adding listener to button for:', button.getAttribute('data-person'));
            button.addEventListener('click', function() {
                const personName = this.getAttribute('data-person');
                console.log('View profile button clicked for:', personName);
                viewProfile(personName);
            });
        });
        
    } catch (error) {
        console.error('Error loading network:', error);
        feedContent.innerHTML = '<p class="text-center text-red-500 py-8">Error loading network</p>';
    }
    hideLoading();
}

// Disconnect Person Function - Using soft delete approach (mark as inactive)
// TypeQL delete operations are incompatible with this TypeDB setup
window.disconnectPerson = async function(personName) {
    console.log('disconnectPerson called with:', personName);
    
    // Show confirmation dialog
    if (!confirm(`Are you sure you want to disconnect from ${personName}?`)) {
        return;
    }
    
    showLoading();
    
    try {
        // Convert personName to username format (lowercase with underscores)
        const username = personName.toLowerCase().replace(/\s+/g, '_');
        
        // Get current user dynamically
        const currentUser = await getCurrentUser();
        if (!currentUser) {
            console.error('No current user found for disconnect');
            return;
        }
        
        // Use proper TypeQL 3.0 syntax with links and match the same 'name' attribute used in Network query
        const query = `
            match
            $me isa person, has name "${currentUser.name}";
            $other isa person, has name "${personName}";
            $friendship_to_delete isa friendship;
            $friendship_to_delete links (friend: $me, friend: $other);
            delete
            $friendship_to_delete;
        `;
        console.log('Disconnect (soft delete) query being sent:', query);
        console.log('Username converted to:', username);
        
        const disconnectData = await makeQuery('persons', query, 'Write');
        
        if (disconnectData.ok) {
            showNotification(`Disconnected from ${personName}`, 'success');
            console.log(`Successfully disconnected from ${personName} - they should now appear in Find Connections`);
            // Refresh the network to update the list
            await loadNetwork();
            // Also refresh suggested connections to potentially show them again
            await loadSuggestedConnections();
            // If we're on Find Connections page, refresh it too
            if (currentView === 'find-connections') {
                await findConnections();
            }
        } else {
            showNotification('Failed to disconnect', 'error');
        }
    } catch (error) {
        console.error('Error disconnecting person:', error);
        showNotification('Error disconnecting from person', 'error');
    }
    
    hideLoading();
}

async function loadJobs() {
    showLoading();
    
    const jobsHTML = `
        <div class="space-y-6">
            <div class="profile-card p-6">
                <h3 class="text-xl font-semibold text-gray-900 mb-4">Recommended Jobs</h3>
                <div class="space-y-4">
                    <div class="border-l-4 border-blue-500 pl-4">
                        <h4 class="font-semibold text-gray-900">Senior Software Engineer</h4>
                        <p class="text-gray-600">Google Inc.</p>
                        <p class="text-sm text-gray-500 mt-1">San Francisco, CA • Full-time</p>
                        <p class="text-sm text-gray-700 mt-2">Join our team working on cutting-edge ML infrastructure...</p>
                        <div class="mt-3">
                            <span class="connection-badge text-white px-2 py-1 rounded-full text-xs">2nd degree connection</span>
                        </div>
                    </div>
                    <div class="border-l-4 border-purple-500 pl-4">
                        <h4 class="font-semibold text-gray-900">Engineering Manager</h4>
                        <p class="text-gray-600">Microsoft Corp.</p>
                        <p class="text-sm text-gray-500 mt-1">Seattle, WA • Full-time</p>
                        <p class="text-sm text-gray-700 mt-2">Lead a team of talented engineers building cloud solutions...</p>
                        <div class="mt-3">
                            <span class="company-badge text-white px-2 py-1 rounded-full text-xs">Direct connection</span>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    `;
    
    feedContent.innerHTML = jobsHTML;
    hideLoading();
}

async function loadAnalytics() {
    showLoading();
    try {
        // Get current user dynamically
        const currentUser = await getCurrentUser();
        if (!currentUser) {
            console.error('No current user found for analytics');
            hideLoading();
            return;
        }
        
        // Get companies that current user is following
        const followingData = await makeQuery('organizations', `
            match 
            $person isa person, has name "${currentUser.name}";
            $company isa company, has name $company_name;
            $following (follower: $person, page: $company) isa following;
            select $company, $company_name;
        `);
        
        const followedCompanies = [];
        if (followingData.ok && followingData.ok.answers) {
            followingData.ok.answers.forEach(answer => {
                const companyName = String(answer.data.company_name?.value || answer.data.company_name || '');
                if (companyName) {
                    followedCompanies.push(companyName);
                }
            });
        }
        
        // Get detailed career path data for followed companies
        const careerPathPromises = followedCompanies.map(async (companyName) => {
            // Get 2nd degree connections to company
            const pathData = await makeQuery('persons', `
                match 
                $me isa person, has name "${currentUser.name}"; 
                $path1 isa friendship;
                $path1 links (friend: $me, friend: $friend1);
                $path2 isa friendship;
                $path2 links (friend: $friend1, friend: $friend2);
                $job isa employment;
                $job links (employee: $friend2, employer: $target);
                $target has name "${companyName}"; 
                select $friend1, $friend2, $target;
            `);
            
            // Get direct connections to company
            const directPathData = await makeQuery('persons', `
                match 
                $me isa person, has name "${currentUser.name}"; 
                $friendship isa friendship;
                $friendship links (friend: $me, friend: $friend);
                $job isa employment;
                $job links (employee: $friend, employer: $target);
                $target has name "${companyName}"; 
                select $friend, $target;
            `);
            
            // Get job roles at the company
            const jobRolesData = await makeQuery('persons', `
                match 
                $me isa person, has name "${currentUser.name}"; 
                $path1 isa friendship;
                $path1 links (friend: $me, friend: $friend1);
                $path2 isa friendship;
                $path2 links (friend: $friend1, friend: $friend2);
                $emp_rel isa employment;
                $emp_rel links (employee: $friend2, employer: $target);
                $target has name "${companyName}";
                $emp_rel has description $role;
                select $friend2, $role;
            `);
            
            // Process job roles
            const roles = new Set();
            const connections = new Map();
            
            if (jobRolesData.ok?.answers) {
                jobRolesData.ok.answers.forEach(answer => {
                    const role = answer.data.role?.value || answer.data.role || 'Unknown Role';
                    const personId = answer.data.friend2?.iid || JSON.stringify(answer.data.friend2);
                    roles.add(role);
                    connections.set(personId, role);
                });
            }
            
            // Deduplicate path data by person ID
            const uniquePaths = [];
            const seenPathPersons = new Set();
            if (pathData.ok?.answers) {
                pathData.ok.answers.forEach(answer => {
                    const personId = answer.data.friend2?.iid || JSON.stringify(answer.data.friend2);
                    if (!seenPathPersons.has(personId)) {
                        seenPathPersons.add(personId);
                        uniquePaths.push(answer);
                    }
                });
            }
            
            // Deduplicate direct path data by person ID
            const uniqueDirectPaths = [];
            const seenDirectPersons = new Set();
            if (directPathData.ok?.answers) {
                directPathData.ok.answers.forEach(answer => {
                    const personId = answer.data.friend?.iid || JSON.stringify(answer.data.friend);
                    if (!seenDirectPersons.has(personId)) {
                        seenDirectPersons.add(personId);
                        uniqueDirectPaths.push(answer);
                    }
                });
            }
            
            return {
                company: companyName,
                paths: uniquePaths.length,
                directPaths: uniqueDirectPaths.length,
                roles: Array.from(roles),
                totalConnections: connections.size,
                connectionDetails: uniquePaths
            };
        });
        
        const careerPaths = await Promise.all(careerPathPromises);
        
        // Calculate totals from company-specific data for consistency
        const totalCareerPaths = careerPaths.reduce((sum, cp) => sum + cp.paths + cp.directPaths, 0);
        const totalDirectCareerPaths = careerPaths.reduce((sum, cp) => sum + cp.directPaths, 0);
        const totalSecondDegreeCareerPaths = careerPaths.reduce((sum, cp) => sum + cp.paths, 0);
        
        // Get overall network stats (separate from career paths) - aligned with Network UI query
        const directConnectionsData = await makeQuery('persons', `
            match 
            $me isa person, has name "${currentUser.name}"; 
            $network_relation_xyz (friend: $me, friend: $friend) isa friendship; 
            $friend has name $friend_name;
            not { $friend is $me; };
            select $friend, $friend_name;
        `);
        
        const secondDegreeData = await makeQuery('persons', `
            match 
            $me isa person, has name "${currentUser.name}"; 
            $path1 isa friendship;
            $path1 links (friend: $me, friend: $friend1);
            $path2 isa friendship;
            $path2 links (friend: $friend1, friend: $friend2);
            not { 
                $direct isa friendship;
                $direct links (friend: $me, friend: $friend2);
            };
            select $friend2;
        `);
        
        // Remove duplicates for accurate counts
        const uniqueSecondDegree = [];
        const seenSecondDegree = new Set();
        if (secondDegreeData.ok?.answers) {
            secondDegreeData.ok.answers.forEach(answer => {
                const friendId = answer.data.friend2?.iid || JSON.stringify(answer.data.friend2);
                if (!seenSecondDegree.has(friendId)) {
                    seenSecondDegree.add(friendId);
                    uniqueSecondDegree.push(answer);
                }
            });
        }
        
        // Deduplicate direct connections by ID (accurate data integrity)
        const uniqueDirectConnections = [];
        const seenDirectConnections = new Set();
        if (directConnectionsData.ok?.answers) {
            directConnectionsData.ok.answers.forEach(answer => {
                const friendId = answer.data.friend?.iid || JSON.stringify(answer.data.friend);
                if (!seenDirectConnections.has(friendId)) {
                    seenDirectConnections.add(friendId);
                    uniqueDirectConnections.push(answer);
                }
            });
        }
        
        const directConnections = uniqueDirectConnections.length;
        const secondDegreeConnections = uniqueSecondDegree.length;
        
        // Calculate connection strength score
        const connectionStrengthScore = Math.round(
            (totalDirectCareerPaths * 10 + careerPaths.reduce((sum, cp) => sum + cp.paths, 0) * 5) / 
            Math.max(followedCompanies.length, 1)
        );
        
        const analyticsHTML = `
            <div class="space-y-6">
                <div class="profile-card p-6">
                    <h3 class="text-xl font-semibold text-gray-900 mb-4">Network Analytics</h3>
                    
                    <div class="grid grid-cols-1 md:grid-cols-4 gap-4 mb-6">
                        <div class="text-center p-4 bg-blue-50 rounded-lg">
                            <div class="text-2xl font-bold text-blue-600">${totalCareerPaths}</div>
                            <div class="text-sm text-gray-600">Career Paths</div>
                        </div>
                        <div class="text-center p-4 bg-green-50 rounded-lg">
                            <div class="text-2xl font-bold text-green-600">${directConnections}</div>
                            <div class="text-sm text-gray-600">Direct Connections</div>
                        </div>
                        <div class="text-center p-4 bg-purple-50 rounded-lg">
                            <div class="text-2xl font-bold text-purple-600">${secondDegreeConnections}</div>
                            <div class="text-sm text-gray-600">2nd Degree</div>
                        </div>
                        <div class="text-center p-4 bg-orange-50 rounded-lg">
                            <div class="text-2xl font-bold text-orange-600">${connectionStrengthScore}</div>
                            <div class="text-sm text-gray-600">Network Score</div>
                        </div>
                    </div>
                    
                    <div class="border-t pt-4">
                        <h4 class="font-semibold text-gray-900 mb-3">Career Path Opportunities</h4>
                        ${careerPaths.length > 0 ? careerPaths.map(cp => `
                            <div class="bg-gradient-to-r from-green-50 to-blue-50 rounded-lg p-4 mb-4 border border-green-200">
                                <div class="flex items-center justify-between mb-3">
                                    <div class="flex items-center">
                                        <i class="fas fa-building text-blue-600 mr-2"></i>
                                        <span class="font-semibold text-lg">${cp.company}</span>
                                    </div>
                                    <div class="flex space-x-2">
                                        ${cp.directPaths > 0 ? `<span class="bg-green-100 text-green-800 px-2 py-1 rounded-full text-xs font-medium">Direct: ${cp.directPaths}</span>` : ''}
                                        ${cp.paths > 0 ? `<span class="bg-blue-100 text-blue-800 px-2 py-1 rounded-full text-xs font-medium">2nd Degree: ${cp.paths}</span>` : ''}
                                    </div>
                                </div>
                                
                                <div class="grid grid-cols-1 md:grid-cols-2 gap-4 mb-3">
                                    <div>
                                        <h5 class="font-medium text-gray-800 mb-2 flex items-center">
                                            <i class="fas fa-users text-gray-600 mr-1"></i>
                                            Connection Strength
                                        </h5>
                                        <div class="space-y-1">
                                            ${cp.directPaths > 0 ? `
                                                <div class="flex items-center justify-between bg-green-50 px-2 py-1 rounded">
                                                    <span class="text-sm text-green-700">Direct connections</span>
                                                    <span class="font-medium text-green-800">${cp.directPaths}</span>
                                                </div>
                                            ` : ''}
                                            ${cp.paths > 0 ? `
                                                <div class="flex items-center justify-between bg-blue-50 px-2 py-1 rounded">
                                                    <span class="text-sm text-blue-700">Friend-of-friend</span>
                                                    <span class="font-medium text-blue-800">${cp.paths}</span>
                                                </div>
                                            ` : ''}
                                        </div>
                                    </div>
                                    
                                    <div>
                                        <h5 class="font-medium text-gray-800 mb-2 flex items-center">
                                            <i class="fas fa-briefcase text-gray-600 mr-1"></i>
                                            Available Roles
                                        </h5>
                                        <div class="flex flex-wrap gap-1">
                                            ${cp.roles.length > 0 ? cp.roles.slice(0, 3).map(role => `
                                                <span class="bg-purple-100 text-purple-800 px-2 py-1 rounded text-xs">${role}</span>
                                            `).join('') : '<span class="text-gray-500 text-xs">No role data</span>'}
                                            ${cp.roles.length > 3 ? `<span class="text-gray-500 text-xs">+${cp.roles.length - 3} more</span>` : ''}
                                        </div>
                                    </div>
                                </div>
                                
                                <div class="border-t border-green-200 pt-3">
                                    <div class="flex items-center justify-between">
                                        <p class="text-sm text-gray-600">
                                            <i class="fas fa-lightbulb text-yellow-500 mr-1"></i>
                                            ${cp.directPaths > 0 ? 
                                                `You have direct connections at ${cp.company}! Consider reaching out directly.` :
                                                cp.paths > 0 ? 
                                                    `${cp.paths} potential introduction paths available through mutual connections.` :
                                                    `No current connections found at ${cp.company}. Keep networking to build connections here.`
                                            }
                                        </p>
                                        ${(cp.directPaths > 0 || cp.paths > 0) ? `
                                            <button class="text-blue-600 hover:text-blue-800 text-sm font-medium px-3 py-1 border border-blue-300 rounded hover:bg-blue-50 transition-colors view-connections-btn" data-company="${cp.company}">
                                                View Connections
                                            </button>
                                        ` : ''}
                                    </div>
                                </div>
                            </div>
                        `).join('') : `
                            <div class="bg-gradient-to-r from-gray-50 to-blue-50 rounded-lg p-4">
                                <div class="flex items-center mb-2">
                                    <i class="fas fa-info-circle text-blue-500 mr-2"></i>
                                    <span class="font-medium">Follow Companies to See Career Paths</span>
                                </div>
                                <p class="text-sm text-gray-600">Start following companies you're interested in to see your connection paths and career opportunities!</p>
                            </div>
                        `}
                    </div>
                    
                    <!-- Networking Insights Section -->
                    <div class="border-t pt-6 mt-6">
                        <h4 class="font-semibold text-gray-900 mb-4 flex items-center">
                            <i class="fas fa-chart-line text-indigo-600 mr-2"></i>
                            Networking Insights
                        </h4>
                        
                        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                            <!-- Network Growth -->
                            <div class="bg-white border border-gray-200 rounded-lg p-4">
                                <h5 class="font-medium text-gray-800 mb-3 flex items-center">
                                    <i class="fas fa-trending-up text-green-500 mr-2"></i>
                                    Network Growth
                                </h5>
                                <div class="space-y-2">
                                    <div class="flex justify-between items-center">
                                        <span class="text-sm text-gray-600">Total connections</span>
                                        <span class="font-medium">${directConnections + secondDegreeConnections}</span>
                                    </div>
                                    <div class="flex justify-between items-center">
                                        <span class="text-sm text-gray-600">Companies followed</span>
                                        <span class="font-medium">${followedCompanies.length}</span>
                                    </div>
                                    <div class="flex justify-between items-center">
                                        <span class="text-sm text-gray-600">Career opportunities</span>
                                        <span class="font-medium text-green-600">${totalCareerPaths}</span>
                                    </div>
                                </div>
                            </div>
                            
                            <!-- Recommendations -->
                            <div class="bg-white border border-gray-200 rounded-lg p-4">
                                <h5 class="font-medium text-gray-800 mb-3 flex items-center">
                                    <i class="fas fa-lightbulb text-yellow-500 mr-2"></i>
                                    Smart Recommendations
                                </h5>
                                <div class="space-y-2">
                                    ${careerPaths.length === 0 ? `
                                        <div class="text-sm text-gray-600 bg-blue-50 p-2 rounded">
                                            <i class="fas fa-plus-circle text-blue-500 mr-1"></i>
                                            Follow companies you're interested in to unlock career insights
                                        </div>
                                    ` : careerPaths.some(cp => cp.directPaths > 0) ? `
                                        <div class="text-sm text-gray-600 bg-green-50 p-2 rounded">
                                            <i class="fas fa-handshake text-green-500 mr-1"></i>
                                            You have direct connections! Consider reaching out for informational interviews
                                        </div>
                                    ` : `
                                        <div class="text-sm text-gray-600 bg-orange-50 p-2 rounded">
                                            <i class="fas fa-network-wired text-orange-500 mr-1"></i>
                                            Strengthen your network by engaging with 2nd degree connections
                                        </div>
                                    `}
                                    
                                    ${careerPaths.filter(cp => cp.roles.length > 0).length > 0 ? `
                                        <div class="text-sm text-gray-600 bg-purple-50 p-2 rounded">
                                            <i class="fas fa-briefcase text-purple-500 mr-1"></i>
                                            ${careerPaths.filter(cp => cp.roles.length > 0).length} companies have role diversity in your network
                                        </div>
                                    ` : ''}
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        `;
        
        feedContent.innerHTML = analyticsHTML;
        
        // Add event listeners for "View Connections" buttons
        const viewConnectionButtons = document.querySelectorAll('.view-connections-btn');
        viewConnectionButtons.forEach(button => {
            button.addEventListener('click', function() {
                const companyName = this.getAttribute('data-company');
                showConnectionDetails(companyName, careerPaths.find(cp => cp.company === companyName));
            });
        });
        
    } catch (error) {
        console.error('Error loading analytics:', error);
        feedContent.innerHTML = '<p class="text-center text-red-500 py-8">Error loading analytics</p>';
    }
    hideLoading();
}

// Function to show detailed connection information
window.showConnectionDetails = async function(companyName, companyData) {
    if (!companyData || (!companyData.directPaths && !companyData.paths)) {
        showNotification('No connection data available for this company', 'info');
        return;
    }
    
    showLoading();
    
    try {
        const currentUser = await getCurrentUser();
        if (!currentUser) {
            console.error('No current user found');
            hideLoading();
            return;
        }
        
        // Get detailed connection information
        const directConnectionsQuery = await makeQuery('persons', `
            match 
            $me isa person, has name "${currentUser.name}"; 
            $friendship isa friendship;
            $friendship links (friend: $me, friend: $friend);
            $emp_rel isa employment;
            $emp_rel links (employee: $friend, employer: $target);
            $target has name "${companyName}";
            $friend has name $friend_name;
            $emp_rel has description $role;
            select $friend_name, $role;
        `);
        
        const secondDegreeQuery = await makeQuery('persons', `
            match 
            $me isa person, has name "${currentUser.name}"; 
            $path1 isa friendship;
            $path1 links (friend: $me, friend: $mutual);
            $path2 isa friendship;
            $path2 links (friend: $mutual, friend: $connection);
            $emp_rel isa employment;
            $emp_rel links (employee: $connection, employer: $target);
            $target has name "${companyName}";
            $mutual has name $mutual_name;
            $connection has name $connection_name;
            $emp_rel has description $role;
            select $mutual_name, $connection_name, $role;
        `);
        
        let modalContent = `
            <div class="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50" id="connectionModal">
                <div class="bg-white rounded-lg p-6 max-w-2xl w-full mx-4 max-h-[80vh] overflow-y-auto">
                    <div class="flex justify-between items-center mb-4">
                        <h3 class="text-xl font-semibold text-gray-900">Connections at ${companyName}</h3>
                        <button onclick="closeConnectionModal()" class="text-gray-400 hover:text-gray-600">
                            <i class="fas fa-times"></i>
                        </button>
                    </div>
                    
                    <div class="space-y-6">
        `;
        
        // Direct connections section with deduplication
        if (directConnectionsQuery.ok?.answers?.length > 0) {
            // Deduplicate direct connections by person ID
            const uniqueDirectConnections = [];
            const seenDirectIds = new Set();
            
            directConnectionsQuery.ok.answers.forEach(answer => {
                const friendId = answer.data.friend?.iid || JSON.stringify(answer.data.friend);
                if (!seenDirectIds.has(friendId)) {
                    seenDirectIds.add(friendId);
                    uniqueDirectConnections.push(answer);
                }
            });
            
            modalContent += `
                <div>
                    <h4 class="font-medium text-gray-800 mb-3 flex items-center">
                        <i class="fas fa-user-friends text-green-500 mr-2"></i>
                        Direct Connections (${uniqueDirectConnections.length})
                    </h4>
                    <div class="space-y-2">
            `;
            
            uniqueDirectConnections.forEach(answer => {
                const friendName = answer.data.friend_name?.value || answer.data.friend_name || 'Unknown';
                const role = answer.data.role?.value || answer.data.role || 'Unknown Role';
                modalContent += `
                    <div class="flex items-center justify-between bg-green-50 p-3 rounded-lg">
                        <div class="flex items-center">
                            <div class="w-8 h-8 bg-green-500 rounded-full flex items-center justify-center text-white font-medium text-sm mr-3">
                                ${friendName.charAt(0).toUpperCase()}
                            </div>
                            <div>
                                <div class="font-medium text-gray-900">${friendName}</div>
                                <div class="text-sm text-gray-600">${role}</div>
                            </div>
                        </div>
                        <button class="text-green-600 hover:text-green-800 text-sm font-medium px-3 py-1 border border-green-300 rounded hover:bg-green-50">
                            Message
                        </button>
                    </div>
                `;
            });
            
            modalContent += `
                    </div>
                </div>
            `;
        }
        
        // Second degree connections section with deduplication
        if (secondDegreeQuery.ok?.answers?.length > 0) {
            // Deduplicate second degree connections by connection ID
            const uniqueSecondDegreeConnections = [];
            const seenConnectionIds = new Set();
            
            secondDegreeQuery.ok.answers.forEach(answer => {
                const connectionId = answer.data.connection?.iid || JSON.stringify(answer.data.connection);
                if (!seenConnectionIds.has(connectionId)) {
                    seenConnectionIds.add(connectionId);
                    uniqueSecondDegreeConnections.push(answer);
                }
            });
            
            modalContent += `
                <div>
                    <h4 class="font-medium text-gray-800 mb-3 flex items-center">
                        <i class="fas fa-network-wired text-blue-500 mr-2"></i>
                        2nd Degree Connections (${uniqueSecondDegreeConnections.length})
                    </h4>
                    <div class="space-y-2">
            `;
            
            uniqueSecondDegreeConnections.forEach(answer => {
                const mutualName = answer.data.mutual_name?.value || answer.data.mutual_name || 'Unknown';
                const connectionName = answer.data.connection_name?.value || answer.data.connection_name || 'Unknown';
                const role = answer.data.role?.value || answer.data.role || 'Unknown Role';
                modalContent += `
                    <div class="bg-blue-50 p-3 rounded-lg">
                        <div class="flex items-center justify-between mb-2">
                            <div class="flex items-center">
                                <div class="w-8 h-8 bg-blue-500 rounded-full flex items-center justify-center text-white font-medium text-sm mr-3">
                                    ${connectionName.charAt(0).toUpperCase()}
                                </div>
                                <div>
                                    <div class="font-medium text-gray-900">${connectionName}</div>
                                    <div class="text-sm text-gray-600">${role}</div>
                                </div>
                            </div>
                            <button class="text-blue-600 hover:text-blue-800 text-sm font-medium px-3 py-1 border border-blue-300 rounded hover:bg-blue-50">
                                Request Intro
                            </button>
                        </div>
                        <div class="text-xs text-gray-500 flex items-center">
                            <i class="fas fa-arrow-right mr-1"></i>
                            Through ${mutualName}
                        </div>
                    </div>
                `;
            });
            
            modalContent += `
                    </div>
                </div>
            `;
        }
        
        modalContent += `
                    </div>
                    
                    <div class="border-t pt-4 mt-6">
                        <div class="flex justify-end space-x-3">
                            <button onclick="closeConnectionModal()" class="px-4 py-2 text-gray-600 border border-gray-300 rounded hover:bg-gray-50">
                                Close
                            </button>
                            <button class="px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700">
                                Export Connections
                            </button>
                        </div>
                    </div>
                </div>
            </div>
        `;
        
        document.body.insertAdjacentHTML('beforeend', modalContent);
        
    } catch (error) {
        console.error('Error loading connection details:', error);
        showNotification('Error loading connection details', 'error');
    }
    
    hideLoading();
}

// Function to close connection modal
window.closeConnectionModal = function() {
    const modal = document.getElementById('connectionModal');
    if (modal) {
        modal.remove();
    }
}

async function showGroups() {
    setActiveNav('groups');
    currentView = 'groups';
    showLoading();
    
    try {
        const currentUser = await getCurrentUser();
        if (!currentUser) {
            console.error('No current user found for groups');
            hideLoading();
            return;
        }
        
        // Get all groups and user's memberships
        const [allGroups, userGroups] = await Promise.all([
            getAllGroups(),
            getUserGroups(currentUser.username)
        ]);
        
        const userGroupIds = new Set(userGroups.map(g => g.groupId));
        const availableGroups = allGroups.filter(group => !userGroupIds.has(group.groupId));
        
        let groupsHTML = `
            <div class="max-w-4xl mx-auto">
                <!-- Header Section -->
                <div class="mb-6">
                    <div class="flex items-center justify-between mb-4">
                        <div>
                            <h2 class="text-2xl font-semibold text-neutral-900 mb-1">Groups</h2>
                            <p class="text-neutral-600">About ${allGroups.length + userGroups.length} results</p>
                        </div>
                        <button id="createGroupBtn" class="btn-primary">
                            <i class="fas fa-plus mr-2"></i>Create Group
                        </button>
                    </div>
                </div>
                
                <!-- Groups List -->
                <div class="space-y-4">
        `;
        
        // Combine all groups for LinkedIn-style vertical list
        const allGroupsForDisplay = [
            ...userGroups.map(group => ({ ...group, isMember: true })),
            ...availableGroups.map(group => ({ ...group, isMember: false }))
        ];
        
        if (allGroupsForDisplay.length > 0) {
            for (const group of allGroupsForDisplay) {
                // Get actual member count from database
                const memberCount = await getGroupMemberCount(group.groupId);
                const groupDescription = getGroupDescription(group.name);
                const isPrivate = group.visibility === 'private';
                
                groupsHTML += `
                    <div class="bg-white border border-neutral-200 rounded-lg p-6 hover:shadow-sm transition-shadow duration-150">
                        <div class="flex items-start justify-between">
                            <div class="flex items-start space-x-4 flex-1">
                                <div class="w-16 h-16 rounded-lg bg-gradient-to-br from-blue-500 to-blue-600 text-white font-bold text-lg flex items-center justify-center flex-shrink-0">
                                    ${group.name.split(' ').map(word => word[0]).join('').toUpperCase().slice(0, 2)}
                                </div>
                                <div class="flex-1 min-w-0">
                                    <h3 class="text-lg font-semibold text-neutral-900 mb-1">${group.name}</h3>
                                    <div class="flex items-center space-x-2 text-sm text-neutral-600 mb-3">
                                        <span class="font-medium">${memberCount} member${memberCount !== 1 ? 's' : ''}</span>
                                        ${group.isMember && group.rank ? `
                                            <span class="text-neutral-400">•</span>
                                            <span class="text-xs px-2 py-1 rounded-full ${group.rank === 'owner' ? 'bg-purple-100 text-purple-800' : 
                                                group.rank === 'admin' ? 'bg-red-100 text-red-800' : 
                                                group.rank === 'moderator' ? 'bg-blue-100 text-blue-800' : 'bg-green-100 text-green-800'}">${group.rank}</span>
                                        ` : ''}
                                    </div>
                                    <p class="text-sm text-neutral-600 leading-relaxed">${groupDescription}</p>
                                </div>
                            </div>
                            <div class="ml-6 flex-shrink-0">
                                ${group.isMember ? `
                                    <div class="flex items-center space-x-2">
                                        <button class="btn-secondary view-group-btn" data-group-id="${group.groupId}">
                                            View
                                        </button>
                                        ${group.rank !== 'owner' ? `
                                            <button class="btn-outline-danger leave-group-btn" data-group-id="${group.groupId}">
                                                Leave
                                            </button>
                                        ` : ''}
                                    </div>
                                ` : `
                                    <button class="btn-primary join-group-btn" data-group-id="${group.groupId}">
                                        Join
                                    </button>
                                `}
                            </div>
                        </div>
                    </div>
                `;
            }
        } else {
            groupsHTML += `
                <div class="bg-white border border-neutral-200 rounded-lg p-12 text-center">
                    <div class="w-16 h-16 bg-neutral-100 rounded-full flex items-center justify-center mx-auto mb-4">
                        <i class="fas fa-layer-group text-neutral-400 text-2xl"></i>
                    </div>
                    <h3 class="text-lg font-semibold text-neutral-900 mb-2">No groups found</h3>
                    <p class="text-neutral-600 mb-4">Create a new group to start building your community.</p>
                </div>
            `;
        }
        
        groupsHTML += `
                </div>
            </div>
        `;
        
        feedContent.innerHTML = groupsHTML;
        
        // Add event listeners
        setupGroupEventListeners();
        
    } catch (error) {
        console.error('Error loading groups:', error);
        feedContent.innerHTML = `
            <div class="post-card text-center py-16">
                <div class="w-16 h-16 bg-red-100 rounded-full flex items-center justify-center mx-auto mb-4">
                    <i class="fas fa-exclamation-triangle text-red-600 text-2xl"></i>
                </div>
                <h3 class="text-heading-3 mb-2">Unable to load groups</h3>
                <p class="text-body mb-4" style="color: var(--neutral-600);">There was an error loading the groups. Please try again.</p>
                <button onclick="showGroups()" class="btn-primary">Try Again</button>
            </div>
        `;
    }
    
    hideLoading();
}

// Setup event listeners for group interactions
function setupGroupEventListeners() {
    // Create group button
    const createGroupBtn = document.getElementById('createGroupBtn');
    if (createGroupBtn) {
        createGroupBtn.addEventListener('click', showCreateGroupModal);
    }
    
    // Join group buttons
    const joinButtons = document.querySelectorAll('.join-group-btn');
    joinButtons.forEach(button => {
        button.addEventListener('click', async function() {
            const groupId = this.getAttribute('data-group-id');
            this.disabled = true;
            this.innerHTML = '<i class="fas fa-spinner fa-spin mr-1"></i>Joining...';
            
            const success = await joinGroup(groupId);
            if (success) {
                await showGroups();
            } else {
                this.disabled = false;
                this.innerHTML = '<i class="fas fa-plus mr-1"></i>Join Group';
            }
        });
    });
    
    // Leave group buttons
    const leaveButtons = document.querySelectorAll('.leave-group-btn');
    leaveButtons.forEach(button => {
        button.addEventListener('click', async function() {
            const groupId = this.getAttribute('data-group-id');
            
            if (confirm('Are you sure you want to leave this group?')) {
                this.disabled = true;
                this.innerHTML = '<i class="fas fa-spinner fa-spin"></i>Leaving...';
                
                const success = await leaveGroup(groupId);
                if (success) {
                    await showGroups();
                } else {
                    this.disabled = false;
                    this.innerHTML = 'Leave';
                }
            }
        });
    });
    
    // View group buttons
    const viewButtons = document.querySelectorAll('.view-group-btn');
    viewButtons.forEach(button => {
        button.addEventListener('click', function() {
            const groupId = this.getAttribute('data-group-id');
            showNotification('Group-specific pages coming soon!', 'info');
        });
    });
}

// Show create group modal
function showCreateGroupModal() {
    const modal = document.createElement('div');
    modal.className = 'fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50';
    modal.innerHTML = `
        <div class="bg-white rounded-lg p-6 w-full max-w-md mx-4">
            <div class="flex items-center justify-between mb-4">
                <h3 class="text-lg font-semibold text-neutral-900">Create New Group</h3>
                <button id="closeModal" class="text-neutral-400 hover:text-neutral-600">
                    <i class="fas fa-times"></i>
                </button>
            </div>
            
            <form id="createGroupForm">
                <div class="mb-4">
                    <label class="block text-sm font-medium text-neutral-700 mb-2">Group Name</label>
                    <input type="text" id="groupName" class="w-full px-3 py-2 border border-neutral-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500" 
                           placeholder="Enter group name..." maxlength="100" required>
                </div>
                
                <div class="mb-6">
                    <label class="block text-sm font-medium text-neutral-700 mb-2">Visibility</label>
                    <select id="groupVisibility" class="w-full px-3 py-2 border border-neutral-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500">
                        <option value="public">Public - Anyone can see and join</option>
                        <option value="private">Private - Invite only</option>
                    </select>
                </div>
                
                <div class="flex justify-end space-x-3">
                    <button type="button" id="cancelCreate" class="btn-secondary">Cancel</button>
                    <button type="submit" id="submitCreate" class="btn-primary">Create Group</button>
                </div>
            </form>
        </div>
    `;
    
    document.body.appendChild(modal);
    
    const closeModal = () => {
        document.body.removeChild(modal);
    };
    
    modal.querySelector('#closeModal').addEventListener('click', closeModal);
    modal.querySelector('#cancelCreate').addEventListener('click', closeModal);
    modal.addEventListener('click', (e) => {
        if (e.target === modal) closeModal();
    });
    
    modal.querySelector('#createGroupForm').addEventListener('submit', async (e) => {
        e.preventDefault();
        
        const name = modal.querySelector('#groupName').value.trim();
        const visibility = modal.querySelector('#groupVisibility').value;
        const submitBtn = modal.querySelector('#submitCreate');
        
        if (!name) {
            showNotification('Please enter a group name', 'error');
            return;
        }
        
        submitBtn.disabled = true;
        submitBtn.innerHTML = '<i class="fas fa-spinner fa-spin mr-2"></i>Creating...';
        
        const result = await createGroup(name, visibility);
        if (result.success) {
            closeModal();
            await showGroups();
        } else {
            submitBtn.disabled = false;
            submitBtn.innerHTML = 'Create Group';
        }
    });
    
    modal.querySelector('#groupName').focus();
}

// Get group description based on group name
function getGroupDescription(groupName) {
    const descriptions = {
        'Tech Enthusiasts': 'Navigating the World of Technology, Innovation, and Digital Transformation. The group is committed to welcoming new members and fostering discussions about emerging technologies, startups, and industry trends.',
        'Tech Innovators': 'At Tech Innovators, we believe in technology as a catalyst for change. If the world\'s organizations and institutions were run more effectively, if our leaders made better decisions, if people worked more productively, we believe that technology can make it happen.',
        'AI Researchers': 'AI is an alliance of builders & executives facilitating the LLM & multimodal AI era, sharing tools to scale powerful, cost-effective applications on the journey to AGI. Conversation starters, thought leadership, and networking opportunities.',
        'Database Developers': 'Welcome to Database Developers – The Largest Community for Database Professionals on LinkedIn. With over 2.3 million members, this group is the ultimate destination for database developers, architects, and industry professionals.',
        'Startup Founders': 'Engaging in the exchange of knowledge is a powerful catalyst for personal and collective growth. By delving into the realms of entrepreneurship, technology, funding, management, etc., we open doors to new opportunities and insights.',
        'Open Source Contributors': 'Professionals working with: Open source software, collaborative development, version control, community-driven projects, distributed systems, DevOps practices, cloud computing, and modern software architecture.',
        'Charity Volunteers': 'A community dedicated to making a positive impact through volunteer work, charitable initiatives, and social responsibility. Connect with like-minded individuals who are passionate about giving back to their communities.'
    };
    
    return descriptions[groupName] || `A professional community focused on ${groupName.toLowerCase()} and related topics. Join to connect with industry professionals, share insights, and stay updated on the latest trends and developments.`;
}

// Quick Action Functions
async function findConnections() {
    setActiveNav('find-connections');
    currentView = 'find-connections';
    showLoading();
    
    try {
        // Get current user dynamically
        const currentUser = await getCurrentUser();
        if (!currentUser) {
            console.error('No current user found for find connections');
            hideLoading();
            return;
        }
        
        // Query 1: People you may know WITH employment data (companies)
        const suggestionsWithEmployment = await makeQuery('persons', `match $me isa person, has name "${currentUser.name}"; $friend1 (friend: $me, friend: $mutual) isa friendship; $friend2 (friend: $mutual, friend: $suggestion) isa friendship; $suggestion has name $name; $employment (employee: $suggestion, employer: $company) isa employment, has description $role; $company has name $company_name; not { $direct (friend: $me, friend: $suggestion) isa friendship; }; not { $suggestion is $me; }; select $suggestion, $name, $company_name, $role;`);
        
        // Query 2: People you may know WITHOUT employment data
        const suggestionsWithoutEmployment = await makeQuery('persons', `match $me isa person, has name "${currentUser.name}"; $friend1 (friend: $me, friend: $mutual) isa friendship; $friend2 (friend: $mutual, friend: $suggestion) isa friendship; $suggestion has name $name; not { $direct (friend: $me, friend: $suggestion) isa friendship; }; not { $suggestion is $me; }; not { $employment (employee: $suggestion, employer: $company) isa employment; }; select $suggestion, $name;`);
        
        // Query 3: All people WITH employment data (broader discovery) - includes recently disconnected
        const allPeopleWithEmployment = await makeQuery('persons', `match $person isa person, has name $name; $me isa person, has name "${currentUser.name}"; $employment (employee: $person, employer: $company) isa employment, has description $role; $company has name $company_name; not { $person is $me; }; not { $friendship (friend: $me, friend: $person) isa friendship; }; select $person, $name, $company_name, $role;`);
        
        // Query 4: All people WITHOUT employment data - includes recently disconnected
        const allPeopleWithoutEmployment = await makeQuery('persons', `match $person isa person, has name $name; $me isa person, has name "${currentUser.name}"; not { $person is $me; }; not { $friendship (friend: $me, friend: $person) isa friendship; }; not { $employment (employee: $person, employer: $company) isa employment; }; select $person, $name;`);
        
        let findConnectionsHTML = `
            <div class="max-w-4xl mx-auto px-4">
                <div class="text-center mb-8">
                    <h2 class="text-heading-1 mb-2" style="color: var(--neutral-900);">Find Connections</h2>
                    <p class="text-body" style="color: var(--neutral-600);">Discover and connect with professionals in your network</p>
                </div>
                
                <!-- People You May Know Section -->
                <div class="profile-card card-hover p-6 mb-6">
                    <h3 class="text-body-large font-semibold mb-4" style="color: var(--neutral-900);">People You May Know</h3>
                    <div class="space-y-3">
        `;
        
        // Combine and prioritize suggestions: employment first, then no employment
        const allSuggestions = [];
        const seenIds = new Set();
        
        // Priority 1: People with employment data
        if (suggestionsWithEmployment.ok && suggestionsWithEmployment.ok.answers) {
            suggestionsWithEmployment.ok.answers.forEach(answer => {
                const personId = answer.data.suggestion?.iid || JSON.stringify(answer.data.suggestion);
                if (!seenIds.has(personId)) {
                    seenIds.add(personId);
                    allSuggestions.push({
                        ...answer,
                        priority: 1,
                        hasEmployment: true
                    });
                }
            });
        }
        
        // Priority 2: People without employment data
        if (suggestionsWithoutEmployment.ok && suggestionsWithoutEmployment.ok.answers) {
            suggestionsWithoutEmployment.ok.answers.forEach(answer => {
                const personId = answer.data.suggestion?.iid || JSON.stringify(answer.data.suggestion);
                if (!seenIds.has(personId)) {
                    seenIds.add(personId);
                    allSuggestions.push({
                        ...answer,
                        priority: 2,
                        hasEmployment: false
                    });
                }
            });
        }
        
        // Render prioritized suggestions in list view
        allSuggestions.forEach(suggestion => {
            const name = String(suggestion.data.name?.value || suggestion.data.name || 'Unknown');
            const initials = name.split(' ').map(n => n[0]).join('').toUpperCase();
            const company = suggestion.hasEmployment ? String(suggestion.data.company_name?.value || suggestion.data.company_name || '') : '';
            const role = suggestion.hasEmployment ? String(suggestion.data.role?.value || suggestion.data.role || '') : '';
            
            findConnectionsHTML += `
                <div class="relative p-6 border border-neutral-200 rounded-lg hover:bg-neutral-50 transition-all duration-150">
                    <div class="flex items-center space-x-6 pr-24">
                        <div class="avatar">${initials}</div>
                        <div>
                            <h4 class="text-body font-semibold" style="color: var(--neutral-900);">${name}</h4>
                            ${company ? `<p class="text-caption" style="color: var(--neutral-600);">${role} at ${company}</p>` : `<p class="text-caption" style="color: var(--neutral-500);">Professional</p>`}
                            <p class="text-caption" style="color: var(--neutral-400);">Mutual connection</p>
                        </div>
                    </div>
                    <div class="absolute bottom-3 right-3 flex items-center space-x-2">
                        <button class="connect-person-btn px-3 py-1 bg-blue-600 text-white rounded-full hover:bg-blue-700 transition-colors text-xs font-medium" data-person="${name}">Connect</button>
                        <button class="view-profile-btn px-3 py-1 border border-blue-600 text-blue-600 rounded-full hover:bg-blue-50 transition-colors text-xs font-medium" data-person="${name}">View</button>
                    </div>
                </div>
            `;
        });
        
        findConnectionsHTML += `
                    </div>
                </div>
                
                <!-- More People Section -->
                <div class="profile-card card-hover p-6">
                    <h3 class="text-body-large font-semibold mb-4" style="color: var(--neutral-900);">More People to Connect With</h3>
                    <div class="space-y-3">
        `;
        
        // Combine and prioritize all people: employment first, then no employment
        const allDiscoveryPeople = [];
        const allSeenIds = new Set();
        
        // Priority 1: People with employment data
        if (allPeopleWithEmployment.ok && allPeopleWithEmployment.ok.answers) {
            allPeopleWithEmployment.ok.answers.forEach(answer => {
                const personId = answer.data.person?.iid || JSON.stringify(answer.data.person);
                if (!allSeenIds.has(personId)) {
                    allSeenIds.add(personId);
                    allDiscoveryPeople.push({
                        ...answer,
                        priority: 1,
                        hasEmployment: true
                    });
                }
            });
        }
        
        // Priority 2: People without employment data
        if (allPeopleWithoutEmployment.ok && allPeopleWithoutEmployment.ok.answers) {
            allPeopleWithoutEmployment.ok.answers.forEach(answer => {
                const personId = answer.data.person?.iid || JSON.stringify(answer.data.person);
                if (!allSeenIds.has(personId)) {
                    allSeenIds.add(personId);
                    allDiscoveryPeople.push({
                        ...answer,
                        priority: 2,
                        hasEmployment: false
                    });
                }
            });
        }
        
        // Limit to 8 people and render in list view
        const limitedDiscovery = allDiscoveryPeople.slice(0, 8);
        limitedDiscovery.forEach(person => {
            const name = String(person.data.name?.value || person.data.name || 'Unknown');
            const initials = name.split(' ').map(n => n[0]).join('').toUpperCase();
            const company = person.hasEmployment ? String(person.data.company_name?.value || person.data.company_name || '') : '';
            const role = person.hasEmployment ? String(person.data.role?.value || person.data.role || '') : '';
            
            findConnectionsHTML += `
                <div class="relative p-6 border border-neutral-200 rounded-lg hover:bg-neutral-50 transition-all duration-150">
                    <div class="flex items-center space-x-6 pr-24">
                        <div class="avatar">${initials}</div>
                        <div>
                            <h4 class="text-body font-semibold" style="color: var(--neutral-900);">${name}</h4>
                            ${company ? `<p class="text-caption" style="color: var(--neutral-600);">${role} at ${company}</p>` : `<p class="text-caption" style="color: var(--neutral-500);">Professional</p>`}
                            <p class="text-caption" style="color: var(--neutral-400);">Suggested for you</p>
                        </div>
                    </div>
                    <div class="absolute bottom-3 right-3 flex items-center space-x-2">
                        <button class="connect-person-btn px-3 py-1 bg-blue-600 text-white rounded-full hover:bg-blue-700 transition-colors text-xs font-medium" data-person="${name}">Connect</button>
                        <button class="view-profile-btn px-3 py-1 border border-blue-600 text-blue-600 rounded-full hover:bg-blue-50 transition-colors text-xs font-medium" data-person="${name}">View</button>
                    </div>
                </div>
            `;
        });
        
        findConnectionsHTML += `
                    </div>
                </div>
            </div>
        `;
        
        feedContent.innerHTML = findConnectionsHTML;
        
        // Add event listeners to connect buttons
        const connectButtons = document.querySelectorAll('.connect-person-btn');
        
        // Add event listeners to view profile buttons
        const viewProfileButtons = document.querySelectorAll('.view-profile-btn');
        console.log('Found view profile buttons in Find Connections:', viewProfileButtons.length);
        viewProfileButtons.forEach(button => {
            console.log('Adding listener to button for:', button.getAttribute('data-person'));
            button.addEventListener('click', function() {
                const personName = this.getAttribute('data-person');
                console.log('View profile button clicked for:', personName);
                viewProfile(personName);
            });
        });
        connectButtons.forEach(button => {
            button.addEventListener('click', function() {
                const personName = this.getAttribute('data-person');
                connectPerson(personName);
            });
        });
        
    } catch (error) {
        console.error('Error loading find connections:', error);
        feedContent.innerHTML = '<p class="text-center text-red-500 py-8">Error loading connection suggestions</p>';
    }
    
    hideLoading();
}

async function exploreCompanies() {
    showLoading();
    try {
        const companiesData = await makeQuery('organizations', 'match $company isa company, has name $name; limit 10; select $company, $name;');
        
        // Get current user dynamically
        const currentUser = await getCurrentUser();
        if (!currentUser) {
            console.error('No current user found for companies');
            hideLoading();
            return;
        }
        
        // Get companies that current user is already following
        const followingData = await makeQuery('organizations', `
            match 
            $person isa person, has name "${currentUser.name}";
            $company isa company, has name $company_name;
            $following (follower: $person, page: $company) isa following;
            select $company, $company_name;
        `);
        
        console.log('Following data response:', followingData);
        
        const followedCompanies = new Set();
        if (followingData.ok && followingData.ok.answers) {
            followingData.ok.answers.forEach(answer => {
                const companyName = String(answer.data.company_name?.value || answer.data.company_name || '');
                followedCompanies.add(companyName);
            });
        }
        
        let companiesHTML = '<div class="space-y-4">';
        
        if (companiesData.ok && companiesData.ok.answers && companiesData.ok.answers.length > 0) {
            companiesHTML += companiesData.ok.answers.map(answer => {
                const name = String(answer.data.name?.value || answer.data.name || 'Unknown Company');
                const initials = name.split(' ').map(n => n[0]).join('').toUpperCase().slice(0, 2);
                const isFollowing = followedCompanies.has(name);
                
                const buttonText = isFollowing ? 'Following' : 'Follow';
                const buttonClass = isFollowing 
                    ? 'btn-secondary follow-company-btn'
                    : 'btn-primary follow-company-btn';
                const buttonStyle = isFollowing ? 'background: var(--success-green); color: white; border-color: var(--success-green);' : '';
                
                return `
                    <div class="profile-card card-hover p-5 mb-4">
                        <div class="flex items-center justify-between">
                            <div class="flex items-center space-x-4">
                                <div class="avatar avatar-sm company-badge">${initials}</div>
                                <div class="flex-1">
                                    <div class="text-body-large font-semibold mb-1" style="color: var(--neutral-900);">${name}</div>
                                    <p class="text-body mb-1" style="color: var(--neutral-600);">Technology Company</p>
                                    <p class="text-caption" style="color: var(--neutral-500);">View employees and opportunities</p>
                                </div>
                            </div>
                            <button class="${buttonClass}" style="${buttonStyle} padding: 10px 20px; font-size: 13px;" data-company="${name}">${buttonText}</button>
                        </div>
                    </div>
                `;
            }).join('');
        } else {
            companiesHTML += '<p class="text-center text-gray-500 py-8">No companies found</p>';
        }
        
        companiesHTML += '</div>';
        feedContent.innerHTML = companiesHTML;
        
        // Add event listeners to follow buttons
        const followButtons = document.querySelectorAll('.follow-company-btn');
        followButtons.forEach(button => {
            button.addEventListener('click', function() {
                const companyName = this.getAttribute('data-company');
                followCompany(companyName);
            });
        });
        
    } catch (error) {
        console.error('Error loading companies:', error);
        feedContent.innerHTML = '<p class="text-center text-red-500 py-8">Error loading companies</p>';
    }
    hideLoading();
}

async function careerPaths() {
    showAnalytics();
}

// Connect Person Functions
window.connectPerson = async function(personName) {
    console.log('connectPerson called with:', personName);
    const button = document.querySelector(`button[data-person="${personName}"]`);
    console.log('Button found:', button);
    
    if (!button) {
        console.error('Button not found for person:', personName);
        return;
    }
    
    const isConnected = button.classList.contains('connected');
    console.log('Is connected:', isConnected);
    
    showLoading();
    
    try {
        if (isConnected) {
            // Get current user dynamically
            const currentUser = await getCurrentUser();
            if (!currentUser) {
                console.error('No current user found for disconnect');
                return;
            }
            
            // Disconnect from person
            const disconnectData = await makeQuery('persons', `
                match 
                $me isa person, has name "${currentUser.name}";
                $person isa person, has name "${personName}";
                delete (friend: $me, friend: $person) isa friendship;
            `, 'Write');
            
            if (disconnectData.ok) {
                button.classList.remove('connected', 'bg-green-600', 'hover:bg-green-700');
                button.classList.add('bg-blue-600', 'hover:bg-blue-700');
                button.innerHTML = `
                    <svg class="w-4 h-4 mx-auto" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6v6m0 0v6m0-6h6m-6 0H6"></path>
                    </svg>
                `;
                button.title = 'Connect';
                showNotification(`Disconnected from ${personName}`, 'success');
                // Refresh suggested connections to update the list
                await loadSuggestedConnections();
            }
        } else {
            // Get current user dynamically
            const currentUser = await getCurrentUser();
            if (!currentUser) {
                console.error('No current user found for connect');
                return;
            }
            
            // Connect to person
            const connectData = await makeQuery('persons', `
                match 
                $me isa person, has name "${currentUser.name}";
                $person isa person, has name "${personName}";
                insert (friend: $me, friend: $person) isa friendship;
            `, 'Write');
            
            if (connectData.ok) {
                button.classList.remove('bg-blue-600', 'hover:bg-blue-700');
                button.classList.add('connected', 'bg-green-600', 'hover:bg-green-700');
                button.innerHTML = `
                    <svg class="w-4 h-4 mx-auto" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path>
                    </svg>
                `;
                button.title = 'Connected';
                showNotification(`Now connected to ${personName}`, 'success');
                // Refresh suggested connections to update the list
                await loadSuggestedConnections();
            }
        }
    } catch (error) {
        console.error('Error connecting/disconnecting person:', error);
        showNotification('Error updating connection status', 'error');
    }
    
    hideLoading();
}

// Follow Company Functions
window.followCompany = async function(companyName) {
    console.log('followCompany called with:', companyName);
    const button = document.querySelector(`button[data-company="${companyName}"]`);
    console.log('Button found:', button);
    
    if (!button) {
        console.error('Button not found for company:', companyName);
        return;
    }
    
    const isFollowing = button.textContent.trim() === 'Following';
    console.log('Is following:', isFollowing);
    
    showLoading();
    
    try {
        if (isFollowing) {
            // Unfollow company
            const unfollowData = await makeQuery('organizations', `
                match
                $person isa person, has name "Jason Clark";
                $company isa company, has name "${companyName}";
                $follow_relation isa following;
                $follow_relation links (follower: $person, page: $company);
                delete $follow_relation;
            `, 'Write');
            
            if (unfollowData.ok) {
                button.textContent = 'Follow';
                button.className = 'btn-primary follow-company-btn';
                button.style = '';
                showNotification(`Unfollowed ${companyName}`, 'success');
            }
        } else {
            // Get current user dynamically
            const currentUser = await getCurrentUser();
            if (!currentUser) {
                console.error('No current user found for follow');
                return;
            }
            
            // Follow company
            const followData = await makeQuery('organizations', `
                match 
                $person isa person, has name "${currentUser.name}";
                $company isa company, has name "${companyName}";
                insert (follower: $person, page: $company) isa following;
            `, 'Write');
            
            if (followData.ok) {
                button.textContent = 'Following';
                button.className = 'btn-secondary follow-company-btn';
                button.style = 'background: var(--success-green); color: white; border-color: var(--success-green);';
                showNotification(`Now following ${companyName}`, 'success');
            }
        }
    } catch (error) {
        console.error('Error following/unfollowing company:', error);
        showNotification('Error updating follow status', 'error');
    }
    
    hideLoading();
}

// Notification function
function showNotification(message, type = 'info') {
    const notification = document.createElement('div');
    notification.className = `fixed top-4 right-4 px-4 py-2 rounded-lg text-white z-50 pointer-events-none ${
        type === 'success' ? 'bg-green-500' : 
        type === 'error' ? 'bg-red-500' : 'bg-blue-500'
    }`;
    notification.textContent = message;
    
    document.body.appendChild(notification);
    
    setTimeout(() => {
        notification.remove();
    }, 3000);
}

// Utility Functions
function getTimeAgo(timestamp) {
    if (typeof timestamp === 'string' && !timestamp.includes('T')) {
        return timestamp; // Already formatted like "2 hours ago"
    }
    
    const now = new Date();
    const time = new Date(timestamp);
    const diffInSeconds = Math.floor((now - time) / 1000);
    
    // Handle invalid timestamps
    if (isNaN(diffInSeconds) || diffInSeconds < 0) {
        return 'Unknown time';
    }
    
    if (diffInSeconds < 60) return 'Just now';
    if (diffInSeconds < 3600) return `${Math.floor(diffInSeconds / 60)}m ago`;
    if (diffInSeconds < 86400) return `${Math.floor(diffInSeconds / 3600)}h ago`;
    if (diffInSeconds < 604800) return `${Math.floor(diffInSeconds / 86400)}d ago`; // Up to 7 days
    if (diffInSeconds < 2592000) return `${Math.floor(diffInSeconds / 604800)}w ago`; // Up to 4 weeks
    return `${Math.floor(diffInSeconds / 2592000)}mo ago`; // Months
}

// API Functions
async function makeQuery(endpoint, query, queryType = 'Query') {
    try {
        const serviceMethod = queryType === 'Write' ? 'write' : 'read';
        const response = await fetch(`${API_BASE}/${endpoint}`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'x-service-method': serviceMethod
            },
            body: JSON.stringify({ query })
        });
        
        if (!response.ok) {
            const errorData = await response.json();
            throw new Error(errorData.message || `HTTP ${response.status}`);
        }
        
        const data = await response.json();
        return data;
        
    } catch (err) {
        console.error('Query failed:', err);
        throw err;
    }
}

// Initialize navigation system
function initializeNavigation() {
    // Set initial active navigation
    setActiveNav('feed');
    
    // Load initial content
    loadFeed();
}

// Event Listeners
document.addEventListener('DOMContentLoaded', function() {
    loadUserProfile();
    initializeNavigation();
    console.log('SocialNet app loaded');
    
    // Modal close handler
    modal.addEventListener('click', function(e) {
        if (e.target === modal) {
            hideModal();
        }
    });
    
    // Search functionality
    const searchInput = document.querySelector('input[placeholder="Search people, companies..."]');
    if (searchInput) {
        searchInput.addEventListener('keypress', function(e) {
            if (e.key === 'Enter') {
                performSearch(this.value);
            }
        });
    }
});

async function performSearch(query) {
    if (!query.trim()) return;
    
    showLoading();
    try {
        // Search for people and companies
        const peopleData = await makeQuery('persons', `match $person isa person, has name $name; $name contains "${query}"; limit 5; select $person, $name;`);
        const companiesData = await makeQuery('organizations', `match $company isa company, has name $name; $name contains "${query}"; limit 5; select $company, $name;`);
        
        let searchHTML = '<div class="space-y-6">';
        
        // People results
        if (peopleData.ok && peopleData.ok.answers && peopleData.ok.answers.length > 0) {
            searchHTML += `
                <div class="profile-card p-4">
                    <h3 class="font-semibold text-gray-900 mb-3">People</h3>
                    <div class="space-y-3">
            `;
            
            searchHTML += peopleData.ok.answers.map(answer => {
                const name = String(answer.data.name?.value || answer.data.name || 'Unknown');
                const initials = name.split(' ').map(n => n[0]).join('').toUpperCase();
                
                return `
                    <div class="flex items-center justify-between">
                        <div class="flex items-center space-x-3">
                            <div class="avatar avatar-sm">${initials}</div>
                            <div>
                                <div class="text-body font-semibold" style="color: var(--neutral-900);">${name}</div>
                                <div class="text-gray-500 text-sm">Professional</div>
                            </div>
                        </div>
                        <button class="text-blue-600 hover:text-blue-800 text-sm font-medium">Connect</button>
                    </div>
                `;
            }).join('');
            
            searchHTML += '</div></div>';
        }
        
        // Companies results
        if (companiesData.ok && companiesData.ok.answers && companiesData.ok.answers.length > 0) {
            searchHTML += `
                <div class="profile-card p-4">
                    <h3 class="font-semibold text-gray-900 mb-3">Companies</h3>
                    <div class="space-y-3">
            `;
            
            searchHTML += companiesData.ok.answers.map(answer => {
                const name = String(answer.data.name?.value || answer.data.name || 'Unknown Company');
                const initials = name.split(' ').map(n => n[0]).join('').toUpperCase().slice(0, 2);
                
                return `
                    <div class="flex items-center justify-between">
                        <div class="flex items-center space-x-3">
                            <div class="avatar avatar-sm company-badge">${initials}</div>
                            <div>
                                <div class="text-body font-semibold" style="color: var(--neutral-900);">${name}</div>
                                <div class="text-gray-500 text-sm">Company</div>
                            </div>
                        </div>
                        <button class="text-blue-600 hover:text-blue-800 text-sm font-medium">Follow</button>
                    </div>
                `;
            }).join('');
            
            searchHTML += '</div></div>';
        }
        
        if (!peopleData.ok?.answers?.length && !companiesData.ok?.answers?.length) {
            searchHTML += `
                <div class="profile-card p-8 text-center">
                    <i class="fas fa-search text-gray-400 text-4xl mb-4"></i>
                    <h3 class="text-lg font-medium text-gray-900 mb-2">No results found</h3>
                    <p class="text-gray-500">Try searching for different keywords</p>
                </div>
            `;
        }
        
        searchHTML += '</div>';
        feedContent.innerHTML = searchHTML;
        
    } catch (error) {
        console.error('Search failed:', error);
        feedContent.innerHTML = '<p class="text-center text-red-500 py-8">Search failed</p>';
    }
    hideLoading();
}

// Profile View Functionality
async function viewProfile(personName) {
    console.log('viewProfile called for:', personName);
    showLoading();
    try {
        // Query for detailed person information
        const profileData = await makeQuery('persons', `
            match 
            $person isa person, has name "${personName}";
            $employment (employee: $person, employer: $company) isa employment, has description $role;
            $company has name $company_name;
            select $person, $company_name, $role;
        `);
        
        // Query for person's connections
        const connectionsData = await makeQuery('persons', `
            match 
            $person isa person, has name "${personName}";
            $friendship (friend: $person, friend: $friend) isa friendship;
            $friend has name $friend_name;
            select $friend, $friend_name;
        `);
        
        // Query for person's posts
        const postsData = await makeQuery('posts', `
            match 
            $person isa person, has name "${personName}";
            $posting (author: $person, post: $post) isa posting;
            $post has post-text $text, has creation-timestamp $time;
            select $post, $text, $time;
        `);
        
        // Query to check if this person is already connected to Jason Clark
        const connectionCheckData = await makeQuery('persons', `
            match 
            $me isa person, has name "Jason Clark";
            $person isa person, has name "${personName}";
            $friendship (friend: $me, friend: $person) isa friendship;
            select $friendship;
        `);
        
        const isAlreadyConnected = connectionCheckData.ok && connectionCheckData.ok.answers && connectionCheckData.ok.answers.length > 0;
        
        // Query for mutual connections - find people connected to both Jason Clark and the profile person
        const mutualConnectionsData = await makeQuery('persons', `
            match 
            $me isa person, has name "Jason Clark";
            $person isa person, has name "${personName}";
            $mutual isa person, has name $mutual_name;
            $friendship1 (friend: $me, friend: $mutual) isa friendship;
            $friendship2 (friend: $person, friend: $mutual) isa friendship;
            not { $mutual is $me; };
            not { $mutual is $person; };
            select $mutual, $mutual_name;
        `);
        
        // Query for work history
        const workHistoryData = await makeQuery('persons', `
            match 
            $person isa person, has name "${personName}";
            $employment (employee: $person, employer: $company) isa employment, has description $role;
            $company has name $company_name;
            select $person, $company_name, $role;
        `);
        
        console.log('Profile data loaded, displaying modal');
        displayProfileModal(personName, profileData, connectionsData, postsData, mutualConnectionsData, workHistoryData, isAlreadyConnected);
        
    } catch (error) {
        console.error('Error loading profile:', error);
        showModal('<div class="text-center"><h2 class="text-xl font-semibold text-gray-900 mb-4">Error</h2><p class="text-red-500">Failed to load profile information.</p><button onclick="hideModal()" class="mt-4 px-4 py-2 bg-gray-100 text-gray-700 rounded-lg hover:bg-gray-200">Close</button></div>');
    }
    console.log('viewProfile completed, hiding loading');
    hideLoading();
}

function displayProfileModal(personName, profileData, connectionsData, postsData, mutualConnectionsData, workHistoryData, isAlreadyConnected) {
    const initials = personName.split(' ').map(n => n[0]).join('').toUpperCase();
    
    // Extract employment info
    let currentRole = 'Professional';
    let currentCompany = 'Unknown Company';
    if (profileData.ok && profileData.ok.answers && profileData.ok.answers.length > 0) {
        const employment = profileData.ok.answers[0];
        currentRole = String(employment.data.role?.value || employment.data.role || 'Professional');
        currentCompany = String(employment.data.company_name?.value || employment.data.company_name || 'Unknown Company');
    }
    
    // Deduplicate and count connections by ID
    let uniqueConnectionCount = 0;
    if (connectionsData.ok && connectionsData.ok.answers && connectionsData.ok.answers.length > 0) {
        const seenConnectionIds = new Set();
        for (const connection of connectionsData.ok.answers) {
            const friendId = connection.data.friend?.iid || JSON.stringify(connection.data.friend);
            seenConnectionIds.add(friendId);
        }
        uniqueConnectionCount = seenConnectionIds.size;
    }
    
    // Deduplicate and count posts
    let uniquePostCount = 0;
    if (postsData.ok && postsData.ok.answers && postsData.ok.answers.length > 0) {
        const seenPostTexts = new Set();
        for (const post of postsData.ok.answers) {
            const text = String(post.data.text?.value || post.data.text || 'No content');
            seenPostTexts.add(text);
        }
        uniquePostCount = seenPostTexts.size;
    }
    
    // Generate recent posts with deduplication
    let recentPostsHTML = '';
    if (postsData.ok && postsData.ok.answers && postsData.ok.answers.length > 0) {
        // Deduplicate posts based on text content
        const uniquePosts = [];
        const seenTexts = new Set();
        
        for (const post of postsData.ok.answers) {
            const text = String(post.data.text?.value || post.data.text || 'No content');
            if (!seenTexts.has(text)) {
                seenTexts.add(text);
                uniquePosts.push(post);
            }
        }
        
        recentPostsHTML = uniquePosts.slice(0, 3).map(post => {
            const text = String(post.data.text?.value || post.data.text || 'No content');
            const time = post.data.time?.value || post.data.time || new Date().toISOString();
            const timeAgo = getTimeAgo(time);
            
            return `
                <div class="border-b border-gray-200 pb-3 mb-3 last:border-b-0">
                    <p class="text-gray-800 text-sm">${text}</p>
                    <p class="text-gray-500 text-xs mt-1">${timeAgo}</p>
                </div>
            `;
        }).join('');
    } else {
        recentPostsHTML = '<p class="text-gray-500 text-sm">No recent posts</p>';
    }
    
    // Generate connections list with deduplication
    let connectionsHTML = '';
    if (connectionsData.ok && connectionsData.ok.answers && connectionsData.ok.answers.length > 0) {
        // Deduplicate connections based on friend name
        const uniqueConnections = [];
        const seenIds = new Set();
        
        for (const connection of connectionsData.ok.answers) {
            const friendId = connection.data.friend?.iid || JSON.stringify(connection.data.friend);
            if (!seenIds.has(friendId)) {
                seenIds.add(friendId);
                uniqueConnections.push(connection);
            }
        }
        
        // Update connection count to reflect unique connections
        const uniqueConnectionCount = uniqueConnections.length;
        
        connectionsHTML = uniqueConnections.slice(0, 5).map(connection => {
            const friendName = String(connection.data.friend_name?.value || connection.data.friend_name || 'Unknown');
            const friendInitials = friendName.split(' ').map(n => n[0]).join('').toUpperCase();
            
            return `
                <div class="flex items-center space-x-2 mb-2">
                    <div class="avatar avatar-xs">${friendInitials}</div>
                    <span class="text-sm text-gray-700">${friendName}</span>
                </div>
            `;
        }).join('');
        
        if (uniqueConnectionCount > 5) {
            connectionsHTML += `<p class="text-xs text-gray-500 mt-2">+${uniqueConnectionCount - 5} more connections</p>`;
        }
    } else {
        connectionsHTML = '<p class="text-gray-500 text-sm">No connections found</p>';
    }
    
    // Generate mutual connections
    let mutualConnectionsHTML = '';
    if (mutualConnectionsData.ok && mutualConnectionsData.ok.answers && mutualConnectionsData.ok.answers.length > 0) {
        const uniqueMutuals = [];
        const seenMutualIds = new Set();
        
        for (const mutual of mutualConnectionsData.ok.answers) {
            const mutualId = mutual.data.mutual?.iid || JSON.stringify(mutual.data.mutual);
            const mutualName = String(mutual.data.mutual_name?.value || mutual.data.mutual_name || 'Unknown');
            if (!seenMutualIds.has(mutualId)) {
                seenMutualIds.add(mutualId);
                uniqueMutuals.push(mutual);
            }
        }
        
        mutualConnectionsHTML = uniqueMutuals.slice(0, 3).map(mutual => {
            const mutualName = String(mutual.data.mutual_name?.value || mutual.data.mutual_name || 'Unknown');
            const mutualInitials = mutualName.split(' ').map(n => n[0]).join('').toUpperCase();
            
            return `
                <div class="flex items-center space-x-2 mb-2">
                    <div class="avatar avatar-xs">${mutualInitials}</div>
                    <span class="text-sm text-gray-700">${mutualName}</span>
                </div>
            `;
        }).join('');
        
        if (uniqueMutuals.length > 3) {
            mutualConnectionsHTML += `<p class="text-xs text-gray-500 mt-2">+${uniqueMutuals.length - 3} more mutual connections</p>`;
        }
    } else {
        mutualConnectionsHTML = '<p class="text-gray-500 text-sm">No mutual connections</p>';
    }
    
    // Generate work experience
    let workExperienceHTML = '';
    if (workHistoryData.ok && workHistoryData.ok.answers && workHistoryData.ok.answers.length > 0) {
        const uniqueJobs = [];
        const seenJobs = new Set();
        
        for (const job of workHistoryData.ok.answers) {
            const role = String(job.data.role?.value || job.data.role || 'Professional');
            const company = String(job.data.company_name?.value || job.data.company_name || 'Unknown Company');
            const jobKey = `${role}-${company}`;
            
            if (!seenJobs.has(jobKey)) {
                seenJobs.add(jobKey);
                uniqueJobs.push(job);
            }
        }
        
        workExperienceHTML = uniqueJobs.map(job => {
            const role = String(job.data.role?.value || job.data.role || 'Professional');
            const company = String(job.data.company_name?.value || job.data.company_name || 'Unknown Company');
            
            return `
                <div class="border-b border-gray-200 pb-3 mb-3 last:border-b-0">
                    <h4 class="font-medium text-gray-900">${role}</h4>
                    <p class="text-gray-600 text-sm">${company}</p>
                    <p class="text-gray-500 text-xs">Current Position</p>
                </div>
            `;
        }).join('');
    } else {
        workExperienceHTML = '<p class="text-gray-500 text-sm">No work experience available</p>';
    }
    
    const profileHTML = `
        <div class="max-w-4xl mx-auto">
            <!-- Profile Header -->
            <div class="text-center mb-6">
                <div class="avatar avatar-lg mx-auto mb-4">${initials}</div>
                <h2 class="text-2xl font-bold text-gray-900">${personName}</h2>
                <p class="text-gray-600">${currentRole}</p>
                <p class="text-gray-500">${currentCompany}</p>
                
                <div class="flex justify-center space-x-6 mt-4 text-sm">
                    <div class="text-center">
                        <div class="font-semibold text-gray-900">${uniqueConnectionCount}</div>
                        <div class="text-gray-500">Connections</div>
                    </div>
                    <div class="text-center">
                        <div class="font-semibold text-gray-900">${uniquePostCount}</div>
                        <div class="text-gray-500">Posts</div>
                    </div>
                </div>
            </div>
            
            <!-- About Section -->
            <div class="profile-card p-4 mb-6">
                <h3 class="font-semibold text-gray-900 mb-3">About</h3>
                <p class="text-gray-700 text-sm">
                    ${currentRole} at ${currentCompany}. Active in the professional community with ${uniqueConnectionCount} connections and ${uniquePostCount} posts shared.
                </p>
            </div>
            
            <!-- Profile Content Grid -->
            <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
                <!-- Work Experience -->
                <div class="profile-card p-4">
                    <h3 class="font-semibold text-gray-900 mb-3">Experience</h3>
                    <div class="space-y-3">
                        ${workExperienceHTML}
                    </div>
                </div>
                
                <!-- Recent Posts -->
                <div class="profile-card p-4">
                    <h3 class="font-semibold text-gray-900 mb-3">Recent Posts</h3>
                    <div class="space-y-3">
                        ${recentPostsHTML}
                    </div>
                </div>
                
                <!-- Connections -->
                <div class="profile-card p-4">
                    <h3 class="font-semibold text-gray-900 mb-3">Connections</h3>
                    <div>
                        ${connectionsHTML}
                    </div>
                </div>
            </div>
            
            <!-- Mutual Connections -->
            <div class="profile-card p-4 mt-6">
                <h3 class="font-semibold text-gray-900 mb-3">Mutual Connections</h3>
                <div class="flex flex-wrap gap-4">
                    ${mutualConnectionsHTML}
                </div>
            </div>
            
            <!-- Action Buttons -->
            <div class="flex justify-center space-x-4 mt-6">
                ${isAlreadyConnected ? 
                    `<button class="disconnect-person-modal-btn px-6 py-2 bg-red-600 text-white rounded-lg hover:bg-red-700" data-person="${personName}">Disconnect</button>
                     <button class="px-6 py-2 bg-gray-100 text-gray-700 rounded-lg hover:bg-gray-200">Message</button>` :
                    `<button class="connect-person-btn px-6 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700" data-person="${personName}">Connect</button>
                     <button class="px-6 py-2 bg-gray-100 text-gray-700 rounded-lg hover:bg-gray-200">Message</button>`
                }
            </div>
        </div>
    `;
    
    const fullModalContent = `
        <div class="flex justify-between items-center mb-6">
            <h2 class="text-xl font-semibold text-gray-900">${personName}'s Profile</h2>
            <button onclick="hideModal()" class="text-gray-400 hover:text-gray-600">
                <i class="fas fa-times text-xl"></i>
            </button>
        </div>
        ${profileHTML}
    `;
    showModal(fullModalContent);
    
    // Add event listeners to profile modal buttons
    setTimeout(() => {
        if (!isAlreadyConnected) {
            // Connect button for non-connected people
            const modalConnectButton = document.querySelector('#modal .connect-person-btn[data-person="' + personName + '"]');
            console.log('Looking for modal Connect button:', modalConnectButton);
            if (modalConnectButton) {
                modalConnectButton.addEventListener('click', function() {
                    console.log('Modal Connect button clicked for:', personName);
                    const clickedPersonName = this.getAttribute('data-person');
                    connectPerson(clickedPersonName);
                    hideModal(); // Close modal after connecting
                });
            } else {
                console.error('Modal Connect button not found for:', personName);
            }
        } else {
            // Disconnect button for already connected people
            const modalDisconnectButton = document.querySelector('#modal .disconnect-person-modal-btn[data-person="' + personName + '"]');
            console.log('Looking for modal Disconnect button:', modalDisconnectButton);
            if (modalDisconnectButton) {
                modalDisconnectButton.addEventListener('click', function() {
                    console.log('Modal Disconnect button clicked for:', personName);
                    const clickedPersonName = this.getAttribute('data-person');
                    disconnectPerson(clickedPersonName);
                    hideModal(); // Close modal after disconnecting
                });
            } else {
                console.error('Modal Disconnect button not found for:', personName);
            }
        }
    }, 100);
}
