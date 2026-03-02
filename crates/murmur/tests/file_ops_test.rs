//! Test file size limits

use murmur::{Swarm, FileOps, Error};
use std::path::Path;
use tokio::fs;

#[tokio::test]
async fn test_file_size_limit() {
    // Create a temporary directory
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_path = temp_dir.path().join("storage");
    let test_file = temp_dir.path().join("large_file.bin");

    // Create a file larger than MAX_FILE_SIZE (10 MB)
    let large_content = vec![0u8; 11 * 1024 * 1024]; // 11 MB
    fs::write(&test_file, &large_content).await.unwrap();

    // Create swarm
    let swarm = Swarm::builder()
        .storage_path(storage_path)
        .group_id("test_group")
        .build()
        .await
        .unwrap();

    swarm.start().await.unwrap();

    // Try to upload large file
    let result = swarm.put_file(&test_file).await;

    // Should fail with FileTooLarge error
    match result {
        Err(Error::FileTooLarge { size, max }) => {
            assert_eq!(size, 11 * 1024 * 1024);
            assert_eq!(max, 10 * 1024 * 1024);
            println!("✅ Large file correctly rejected");
        }
        Ok(_) => panic!("Large file should have been rejected"),
        Err(e) => panic!("Unexpected error: {}", e),
    }

    swarm.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_small_file_accepted() {
    // Create a temporary directory
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_path = temp_dir.path().join("storage");
    let test_file = temp_dir.path().join("small_file.txt");

    // Create a small file
    fs::write(&test_file, b"Hello, world!").await.unwrap();

    // Create swarm
    let swarm = Swarm::builder()
        .storage_path(storage_path)
        .group_id("test_group")
        .build()
        .await
        .unwrap();

    swarm.start().await.unwrap();

    // Upload small file
    let result = swarm.put_file(&test_file).await;

    // Should succeed
    match result {
        Ok(key) => {
            println!("✅ Small file accepted: {}", key);

            // Verify metadata
            let meta = swarm.file_metadata(&key).await.unwrap().unwrap();
            assert_eq!(meta.size, 13);
            assert_eq!(meta.version, 1);
        }
        Err(e) => panic!("Small file should have been accepted: {}", e),
    }

    swarm.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_version_conflict_error() {
    // Create a temporary directory
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_path = temp_dir.path().join("storage");
    let test_file = temp_dir.path().join("test.txt");

    fs::write(&test_file, b"Version 1").await.unwrap();

    // Create swarm
    let swarm = Swarm::builder()
        .storage_path(storage_path)
        .group_id("test_group")
        .build()
        .await
        .unwrap();

    swarm.start().await.unwrap();

    // Upload first version
    let key = swarm.put_file(&test_file).await.unwrap();

    // Update file
    fs::write(&test_file, b"Version 2").await.unwrap();

    // Upload second version
    swarm.put_file(&test_file).await.unwrap();

    // Try to upload with old version number
    fs::write(&test_file, b"Version 3").await.unwrap();
    let result = swarm.put_file_with_version(&test_file, Some(1)).await;

    // Should fail with VersionConflict error
    match result {
        Err(Error::VersionConflict { expected, current }) => {
            assert_eq!(expected, 1);
            assert_eq!(current, 2);
            println!("✅ Version conflict correctly detected");
        }
        Ok(_) => panic!("Version conflict should have been detected"),
        Err(e) => panic!("Unexpected error: {}", e),
    }

    swarm.shutdown().await.unwrap();
}
