// API Base URL
const API_BASE = 'http://127.0.0.1:3036';

// Current user data
const currentUser = {
    name: 'Jason Clark',
    username: 'user_2025_17',
    title: 'Software Engineer',
    location: 'San Francisco, CA',
    avatar: 'JC'
};

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
    document.querySelector(`[onclick="show${activeItem.charAt(0).toUpperCase() + activeItem.slice(1)}()"]`).classList.add('active');
}

// Social Media Functions
async function loadFeed() {
    showLoading();
    try {
        // Load posts from the database
        const postsData = await makeQuery('posts', 'match $post isa post, has post-text $text, has creation-timestamp $time; $posting (post: $post, author: $author) isa posting; $author has name $name; limit 10; select $post, $text, $time, $name;');
        
        let feedHTML = '';
        
        if (postsData.ok && postsData.ok.answers && postsData.ok.answers.length > 0) {
            // Remove duplicates based on author name and post text combination
            const uniquePosts = [];
            const seenPosts = new Set();
            
            postsData.ok.answers.forEach(answer => {
                const authorName = String(answer.data.name?.value || answer.data.name || 'Unknown User');
                const postText = String(answer.data.text?.value || answer.data.text || 'No content');
                const postKey = `${authorName}:${postText}`;
                
                if (!seenPosts.has(postKey)) {
                    seenPosts.add(postKey);
                    uniquePosts.push(answer);
                }
            });
            
            feedHTML = uniquePosts.map(answer => {
                // Extract actual values from TypeDB response format
                const authorName = String(answer.data.name?.value || answer.data.name || 'Unknown User');
                const postText = String(answer.data.text?.value || answer.data.text || 'No content');
                const timestamp = answer.data.time?.value || answer.data.time || new Date().toISOString();
                const authorInitials = authorName.split(' ').map(n => n[0]).join('').toUpperCase();
                
                return createPostCard(authorName, authorInitials, postText, timestamp);
            }).join('');
        } else {
            // Show sample posts if no data
            feedHTML = getSamplePosts();
        }
        
        feedContent.innerHTML = feedHTML;
        
        // Load suggested connections and trending topics
        await loadSuggestedConnections();
        await loadTrendingTopics();
        
    } catch (error) {
        console.error('Error loading feed:', error);
        feedContent.innerHTML = getSamplePosts();
    }
    hideLoading();
}

function createPostCard(authorName, authorInitials, content, timestamp) {
    const timeAgo = getTimeAgo(timestamp);
    
    return `
        <div class="post-card p-4">
            <div class="flex items-start space-x-3">
                <div class="avatar">${authorInitials}</div>
                <div class="flex-1">
                    <div class="flex items-center space-x-2 mb-2">
                        <h4 class="font-semibold text-gray-900">${authorName}</h4>
                        <span class="text-gray-500 text-sm">•</span>
                        <span class="text-gray-500 text-sm">${timeAgo}</span>
                    </div>
                    <p class="text-gray-800 mb-3">${content}</p>
                    <div class="flex items-center space-x-6 text-gray-500">
                        <button class="flex items-center space-x-1 hover:text-blue-600">
                            <i class="far fa-thumbs-up"></i>
                            <span class="text-sm">Like</span>
                        </button>
                        <button class="flex items-center space-x-1 hover:text-blue-600">
                            <i class="far fa-comment"></i>
                            <span class="text-sm">Comment</span>
                        </button>
                        <button class="flex items-center space-x-1 hover:text-blue-600">
                            <i class="fas fa-share"></i>
                            <span class="text-sm">Share</span>
                        </button>
                    </div>
                </div>
            </div>
        </div>
    `;
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
        // Find people connected through mutual friends
        const connectionsData = await makeQuery('persons', 'match $me isa person, has name "Jason Clark"; $friend1 (friend: $me, friend: $mutual) isa friendship; $friend2 (friend: $mutual, friend: $suggestion) isa friendship; $suggestion has name $name; not { $direct (friend: $me, friend: $suggestion) isa friendship; }; limit 5; select $suggestion, $name;');
        
        let suggestionsHTML = '';
        
        if (connectionsData.ok && connectionsData.ok.answers && connectionsData.ok.answers.length > 0) {
            // Remove duplicates based on name
            const uniqueConnections = [];
            const seenNames = new Set();
            
            connectionsData.ok.answers.forEach(answer => {
                const name = String(answer.data.name?.value || answer.data.name || 'Unknown User');
                if (!seenNames.has(name)) {
                    seenNames.add(name);
                    uniqueConnections.push(answer);
                }
            });
            
            suggestionsHTML = uniqueConnections.map(answer => {
                const name = String(answer.data.name?.value || answer.data.name || 'Unknown User');
                const initials = name.split(' ').map(n => n[0]).join('').toUpperCase();
                
                return `
                    <div class="flex items-center justify-between">
                        <div class="flex items-center space-x-3">
                            <div class="avatar avatar-sm">${initials}</div>
                            <div>
                                <div class="font-medium text-gray-900 text-sm">${name}</div>
                                <div class="text-gray-500 text-xs">Mutual connection</div>
                            </div>
                        </div>
                        <button class="text-blue-600 hover:text-blue-800 text-sm font-medium">Connect</button>
                    </div>
                `;
            }).join('');
        } else {
            // Show sample suggestions
            suggestionsHTML = getSampleSuggestions();
        }
        
        suggestedConnections.innerHTML = suggestionsHTML;
        
        // Update connection count
        const friendsData = await makeQuery('persons', 'match $me isa person, has name "Jason Clark"; $friendship (friend: $me, friend: $friend) isa friendship; select $friend;');
        const count = friendsData.ok?.answers?.length || 0;
        connectionCount.textContent = count;
        
    } catch (error) {
        console.error('Error loading suggestions:', error);
        suggestedConnections.innerHTML = getSampleSuggestions();
    }
}

function getSampleSuggestions() {
    const suggestions = [
        { name: 'Mia Lewis', initials: 'ML', connection: 'Works at Google' },
        { name: 'Alex Chen', initials: 'AC', connection: 'Manager at Google' },
        { name: 'Sarah Johnson', initials: 'SJ', connection: 'Mutual connection' }
    ];
    
    return suggestions.map(person => `
        <div class="flex items-center justify-between">
            <div class="flex items-center space-x-3">
                <div class="avatar avatar-sm">${person.initials}</div>
                <div>
                    <div class="font-medium text-gray-900 text-sm">${person.name}</div>
                    <div class="text-gray-500 text-xs">${person.connection}</div>
                </div>
            </div>
            <button class="text-blue-600 hover:text-blue-800 text-sm font-medium">Connect</button>
        </div>
    `).join('');
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
                    <div class="text-sm">
                        <div class="font-medium text-gray-900">#${tag}</div>
                        <div class="text-gray-500">${count} post${count !== 1 ? 's' : ''}</div>
                    </div>
                `).join('');
            } else {
                trendingHTML = getSampleTrending();
            }
        } else {
            // Show sample trending if no tags found
            trendingHTML = getSampleTrending();
        }
        
        trendingTopics.innerHTML = trendingHTML;
        
    } catch (error) {
        console.error('Error loading trending topics:', error);
        trendingTopics.innerHTML = getSampleTrending();
    }
}

function getSampleTrending() {
    return `
        <div class="text-sm">
            <div class="font-medium text-gray-900">#TechJobs</div>
            <div class="text-gray-500">No posts yet</div>
        </div>
        <div class="text-sm">
            <div class="font-medium text-gray-900">#RemoteWork</div>
            <div class="text-gray-500">No posts yet</div>
        </div>
        <div class="text-sm">
            <div class="font-medium text-gray-900">#AI</div>
            <div class="text-gray-500">No posts yet</div>
        </div>
    `;
}

async function loadNetwork() {
    showLoading();
    try {
        const networkData = await makeQuery('persons', 'match $me isa person, has name "Jason Clark"; $friendship (friend: $me, friend: $friend) isa friendship; $friend has name $name; $employment (employee: $friend, employer: $company) isa employment, has description $role; $company has name $company_name; select $friend, $name, $company_name, $role;');
        
        let networkHTML = '<div class="space-y-4">';
        
        if (networkData.ok && networkData.ok.answers && networkData.ok.answers.length > 0) {
            // Remove duplicates based on name
            const uniqueConnections = [];
            const seenNames = new Set();
            
            networkData.ok.answers.forEach(answer => {
                const name = String(answer.data.name?.value || answer.data.name || 'Unknown');
                if (!seenNames.has(name)) {
                    seenNames.add(name);
                    uniqueConnections.push(answer);
                }
            });
            
            networkHTML += uniqueConnections.map(answer => {
                const name = String(answer.data.name?.value || answer.data.name || 'Unknown');
                const company = String(answer.data.company_name?.value || answer.data.company_name || 'Unknown Company');
                const role = String(answer.data.role?.value || answer.data.role || 'Unknown Role');
                const initials = name.split(' ').map(n => n[0]).join('').toUpperCase();
                
                return `
                    <div class="profile-card p-4">
                        <div class="flex items-center space-x-4">
                            <div class="avatar">${initials}</div>
                            <div class="flex-1">
                                <h4 class="font-semibold text-gray-900">${name}</h4>
                                <p class="text-gray-600">${role}</p>
                                <p class="text-sm text-gray-500">${company}</p>
                            </div>
                            <div class="flex space-x-2">
                                <button class="px-3 py-1 bg-blue-100 text-blue-700 rounded-full text-sm hover:bg-blue-200">Message</button>
                                <button onclick="viewProfile('${name}')" class="px-3 py-1 border border-gray-300 text-gray-700 rounded-full text-sm hover:bg-gray-50">View Profile</button>
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
        
    } catch (error) {
        console.error('Error loading network:', error);
        feedContent.innerHTML = '<p class="text-center text-red-500 py-8">Error loading network</p>';
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
        // Get real analytics data from TypeDB
        const careerPathData = await makeQuery('persons', 'match $me isa person, has name "Jason Clark"; $path1 (friend: $me, friend: $friend1) isa friendship; $path2 (friend: $friend1, friend: $friend2) isa friendship; $job (employee: $friend2, employer: $target) isa employment; $target has name "Google Inc"; select $friend1, $friend2, $target;');
        
        const directConnectionsData = await makeQuery('persons', 'match $me isa person, has name "Jason Clark"; $friendship (friend: $me, friend: $friend) isa friendship; select $friend;');
        
        const secondDegreeData = await makeQuery('persons', 'match $me isa person, has name "Jason Clark"; $path1 (friend: $me, friend: $friend1) isa friendship; $path2 (friend: $friend1, friend: $friend2) isa friendship; not { $direct (friend: $me, friend: $friend2) isa friendship; }; select $friend2;');
        
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
        
        const pathsToGoogle = careerPathData.ok?.answers?.length || 0;
        const directConnections = directConnectionsData.ok?.answers?.length || 0;
        const secondDegreeConnections = uniqueSecondDegree.length;
        
        const analyticsHTML = `
            <div class="space-y-6">
                <div class="profile-card p-6">
                    <h3 class="text-xl font-semibold text-gray-900 mb-4">Network Analytics</h3>
                    
                    <div class="grid grid-cols-1 md:grid-cols-3 gap-4 mb-6">
                        <div class="text-center p-4 bg-blue-50 rounded-lg">
                            <div class="text-2xl font-bold text-blue-600">${pathsToGoogle}</div>
                            <div class="text-sm text-gray-600">Paths to Google</div>
                        </div>
                        <div class="text-center p-4 bg-green-50 rounded-lg">
                            <div class="text-2xl font-bold text-green-600">${directConnections}</div>
                            <div class="text-sm text-gray-600">Direct Connections</div>
                        </div>
                        <div class="text-center p-4 bg-purple-50 rounded-lg">
                            <div class="text-2xl font-bold text-purple-600">${secondDegreeConnections}</div>
                            <div class="text-sm text-gray-600">2nd Degree</div>
                        </div>
                    </div>
                    
                    <div class="border-t pt-4">
                        <h4 class="font-semibold text-gray-900 mb-3">Career Path Opportunities</h4>
                        <div class="bg-gradient-to-r from-green-50 to-blue-50 rounded-lg p-4">
                            <div class="flex items-center mb-2">
                                <i class="fas fa-route text-green-500 mr-2"></i>
                                <span class="font-medium">Path to Google</span>
                            </div>
                            <p class="text-sm text-gray-600">You have ${pathsToGoogle} friend-of-friend connection(s) to Google employees. This could be valuable for career opportunities!</p>
                        </div>
                    </div>
                </div>
            </div>
        `;
        
        feedContent.innerHTML = analyticsHTML;
        
    } catch (error) {
        console.error('Error loading analytics:', error);
        feedContent.innerHTML = '<p class="text-center text-red-500 py-8">Error loading analytics</p>';
    }
    hideLoading();
}

// Quick Action Functions
async function findConnections() {
    showNetwork();
}

async function exploreCompanies() {
    showLoading();
    try {
        const companiesData = await makeQuery('organizations', 'match $company isa company, has name $name; limit 10; select $company, $name;');
        
        let companiesHTML = '<div class="space-y-4">';
        
        if (companiesData.ok && companiesData.ok.answers && companiesData.ok.answers.length > 0) {
            companiesHTML += companiesData.ok.answers.map(answer => {
                const name = String(answer.data.name?.value || answer.data.name || 'Unknown Company');
                const initials = name.split(' ').map(n => n[0]).join('').toUpperCase().slice(0, 2);
                
                return `
                    <div class="profile-card p-4">
                        <div class="flex items-center space-x-4">
                            <div class="avatar company-badge">${initials}</div>
                            <div class="flex-1">
                                <h4 class="font-semibold text-gray-900">${name}</h4>
                                <p class="text-gray-600">Technology Company</p>
                                <p class="text-sm text-gray-500">View employees and opportunities</p>
                            </div>
                            <button class="px-3 py-1 bg-blue-100 text-blue-700 rounded-full text-sm hover:bg-blue-200">Follow</button>
                        </div>
                    </div>
                `;
            }).join('');
        } else {
            companiesHTML += '<p class="text-center text-gray-500 py-8">No companies found</p>';
        }
        
        companiesHTML += '</div>';
        feedContent.innerHTML = companiesHTML;
        
    } catch (error) {
        console.error('Error loading companies:', error);
        feedContent.innerHTML = '<p class="text-center text-red-500 py-8">Error loading companies</p>';
    }
    hideLoading();
}

async function careerPaths() {
    showAnalytics();
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
        const response = await fetch(`${API_BASE}/${endpoint}`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'x-service-method': 'read'
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

// Event Listeners
document.addEventListener('DOMContentLoaded', function() {
    console.log('SocialNet app loaded');
    
    // Load initial feed
    loadFeed();
    
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
                                <div class="font-medium text-gray-900">${name}</div>
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
                                <div class="font-medium text-gray-900">${name}</div>
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
        
        displayProfileModal(personName, profileData, connectionsData, postsData, mutualConnectionsData, workHistoryData);
        
    } catch (error) {
        console.error('Error loading profile:', error);
        showModal('Error', '<p class="text-red-500">Failed to load profile information.</p>');
    }
    hideLoading();
}

function displayProfileModal(personName, profileData, connectionsData, postsData, mutualConnectionsData, workHistoryData) {
    const initials = personName.split(' ').map(n => n[0]).join('').toUpperCase();
    
    // Extract employment info
    let currentRole = 'Professional';
    let currentCompany = 'Unknown Company';
    if (profileData.ok && profileData.ok.answers && profileData.ok.answers.length > 0) {
        const employment = profileData.ok.answers[0];
        currentRole = String(employment.data.role?.value || employment.data.role || 'Professional');
        currentCompany = String(employment.data.company_name?.value || employment.data.company_name || 'Unknown Company');
    }
    
    // Deduplicate and count connections
    let uniqueConnectionCount = 0;
    if (connectionsData.ok && connectionsData.ok.answers && connectionsData.ok.answers.length > 0) {
        const seenConnectionNames = new Set();
        for (const connection of connectionsData.ok.answers) {
            const friendName = String(connection.data.friend_name?.value || connection.data.friend_name || 'Unknown');
            seenConnectionNames.add(friendName);
        }
        uniqueConnectionCount = seenConnectionNames.size;
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
        const seenNames = new Set();
        
        for (const connection of connectionsData.ok.answers) {
            const friendName = String(connection.data.friend_name?.value || connection.data.friend_name || 'Unknown');
            if (!seenNames.has(friendName)) {
                seenNames.add(friendName);
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
        const seenMutualNames = new Set();
        
        for (const mutual of mutualConnectionsData.ok.answers) {
            const mutualName = String(mutual.data.mutual_name?.value || mutual.data.mutual_name || 'Unknown');
            if (!seenMutualNames.has(mutualName)) {
                seenMutualNames.add(mutualName);
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
                <button class="px-6 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700">Connect</button>
                <button class="px-6 py-2 bg-gray-100 text-gray-700 rounded-lg hover:bg-gray-200">Message</button>
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
}
